from __future__ import annotations

import queue
import time
from concurrent.futures import ThreadPoolExecutor
from typing import Callable

from gui.protocol_client import ControlResponse, ProtocolClient
from gui.response_parser import parse_exec_start_payload, split_stream_payload
from gui.services.session_state import GuiSessionState
from gui.services.ui_events import UiEvent


class RequestHandler:
    def __init__(
        self,
        client: ProtocolClient,
        ui_queue: queue.Queue[UiEvent],
        session: GuiSessionState,
        update_client_config: Callable[[], None],
        kernel_is_running: Callable[[], bool],
    ):
        self._client = client
        self._ui_queue = ui_queue
        self._session = session
        self._update_client_config = update_client_config
        self._kernel_is_running = kernel_is_running
        self._executor = ThreadPoolExecutor(max_workers=4)

    def shutdown(self):
        self._executor.shutdown(wait=False, cancel_futures=True)

    def dispatch_control_request(
        self,
        *,
        verb: str,
        payload: str,
        show_error_popup: bool = True,
        success_event_kind: str | None = None,
        read_timeout_s: float | None = None,
        inactivity_timeout_s: float | None = None,
        include_control_log: bool = True,
        on_done: Callable[[], None] | None = None,
    ):
        def task():
            try:
                self._update_client_config()
                response = self.send_once_with_retry(
                    verb=verb,
                    payload=payload,
                    read_timeout_s=read_timeout_s,
                    inactivity_timeout_s=inactivity_timeout_s,
                )
                if not response.ok:
                    message = f"{verb} failed [{response.code}]: {response.payload}"
                    kind = "error" if show_error_popup else "background_error"
                    self._ui_queue.put(UiEvent(kind=kind, message=message))
                elif success_event_kind:
                    self._ui_queue.put(UiEvent(kind=success_event_kind, message=response.payload))
                if include_control_log:
                    self._ui_queue.put(UiEvent(kind="control", message=self.format_control(verb, response)))
                self._ui_queue.put(
                    UiEvent(kind="protocol_trace", message=f"{verb}\x00{payload}\x00{response.payload}")
                )
            except Exception as exc:
                kind = "error" if show_error_popup else "background_error"
                self._ui_queue.put(UiEvent(kind=kind, message=f"{verb} failed: {exc}"))
            finally:
                if on_done is not None:
                    try:
                        on_done()
                    except Exception:
                        pass

        self._executor.submit(task)

    def refresh_status(self, force: bool = False):
        if self._session.status_in_flight:
            return
        if self._session.load_in_flight and not force:
            return
        should_poll = force or self._session.is_connected or self._kernel_is_running()
        if not should_poll:
            return

        self._session.status_in_flight = True

        def task():
            try:
                self._update_client_config()
                response = self.send_once_with_retry(
                    verb="STATUS",
                    payload="",
                    read_timeout_s=6.0,
                    inactivity_timeout_s=0.35,
                )
                if response.ok:
                    self._ui_queue.put(UiEvent(kind="status", message=response.payload))
                else:
                    self._ui_queue.put(
                        UiEvent(
                            kind="background_error",
                            message=f"STATUS failed [{response.code}]: {response.payload}",
                        )
                    )
            except Exception as exc:
                self._ui_queue.put(UiEvent(kind="background_error", message=f"STATUS failed: {exc}"))
            finally:
                self._session.status_in_flight = False

        self._executor.submit(task)

    def refresh_models(self, silent: bool = False, force: bool = False):
        if self._session.models_in_flight:
            return
        should_refresh = force or self._session.is_connected or self._kernel_is_running() or not silent
        if not should_refresh:
            return

        self._session.models_in_flight = True
        self.dispatch_control_request(
            verb="LIST_MODELS",
            payload="",
            show_error_popup=not silent,
            success_event_kind="models_list",
            read_timeout_s=8.0,
            inactivity_timeout_s=0.6,
            include_control_log=not silent,
            on_done=lambda: setattr(self._session, "models_in_flight", False),
        )

    def refresh_tools(self, silent: bool = False, force: bool = False):
        if self._session.tools_in_flight:
            return
        should_refresh = force or self._session.is_connected or self._kernel_is_running() or not silent
        if not should_refresh:
            return

        self._session.tools_in_flight = True
        self.dispatch_control_request(
            verb="LIST_TOOLS",
            payload="",
            show_error_popup=not silent,
            success_event_kind="tools_list",
            read_timeout_s=8.0,
            inactivity_timeout_s=0.6,
            include_control_log=not silent,
            on_done=lambda: setattr(self._session, "tools_in_flight", False),
        )

    def load_model(self, model_id: str, on_loading: Callable[[str], None]):
        if self._session.load_in_flight or not model_id:
            return

        self._session.load_in_flight = True
        on_loading(model_id)

        def task():
            try:
                self._update_client_config()
                select_response = self.send_once_with_retry(
                    "SELECT_MODEL",
                    model_id,
                    read_timeout_s=8.0,
                    inactivity_timeout_s=0.6,
                )
                if not select_response.ok:
                    raise RuntimeError(
                        f"SELECT_MODEL failed [{select_response.code}]: {select_response.payload}"
                    )
                load_response = self.send_once_with_retry(
                    "LOAD",
                    "",
                    read_timeout_s=self._session.load_read_timeout_s,
                    inactivity_timeout_s=self._session.load_inactivity_timeout_s,
                )
                if not load_response.ok:
                    raise RuntimeError(
                        f"LOAD failed [{load_response.code}]: {load_response.payload}"
                    )
                self._ui_queue.put(UiEvent(kind="refresh_after_load", message=model_id))
            except Exception as exc:
                self._ui_queue.put(UiEvent(kind="error", message=f"LOAD failed: {exc}"))
            finally:
                self._session.load_in_flight = False

        self._executor.submit(task)

    def request_backend_diag(self):
        def task():
            try:
                self._update_client_config()
                response = self.send_once_with_retry(
                    verb="BACKEND_DIAG",
                    payload="",
                    read_timeout_s=8.0,
                    inactivity_timeout_s=0.6,
                )
                kind = "backend_diag" if response.ok else "backend_diag_error"
                self._ui_queue.put(UiEvent(kind=kind, message=response.payload))
                self._ui_queue.put(
                    UiEvent(kind="protocol_trace", message=f"BACKEND_DIAG\x00\x00{response.payload}")
                )
            except Exception as exc:
                self._ui_queue.put(UiEvent(kind="backend_diag_error", message=str(exc)))

        self._executor.submit(task)

    def exec_stream(
        self,
        *,
        prompt: str,
        workload: str,
        has_loaded_model: bool,
        begin_user_message: Callable[[str], int],
        show_error: Callable[[str], None],
    ):
        if not prompt:
            return
        if not has_loaded_model:
            show_error("Nessun modello caricato — usa Models → Load prima di Chat.")
            return

        outbound_prompt = prompt
        if workload and workload != "auto":
            outbound_prompt = f"capability={workload}; {prompt}"

        req_id = begin_user_message(prompt)

        def task():
            pid = 0
            try:
                self._update_client_config()

                def on_frame(kind: str, code: str, body: bytes):
                    nonlocal pid
                    text = body.decode("utf-8", errors="replace")
                    if kind == "+OK":
                        started = parse_exec_start_payload(text)
                        if started is not None:
                            pid = started.pid
                            self._ui_queue.put(
                                UiEvent(kind="exec_started", message=f"{pid}\x00{req_id}")
                            )
                    elif kind == "DATA" and code.lower() == "raw":
                        cleaned_text, marker = split_stream_payload(text)
                        if marker is not None:
                            self._ui_queue.put(
                                UiEvent(
                                    kind="exec_metrics",
                                    message=(
                                        f"{marker.pid}\x00"
                                        f"{marker.tokens_generated}\x00"
                                        f"{marker.elapsed_secs}"
                                    ),
                                )
                            )
                        if cleaned_text:
                            self._ui_queue.put(
                                UiEvent(kind="exec_stream", message=f"{pid}\x00{cleaned_text}")
                            )

                result = self.exec_stream_with_retry(prompt=outbound_prompt, on_frame=on_frame)
                if pid > 0:
                    self._ui_queue.put(UiEvent(kind="exec_done", message=f"{pid}\x00{req_id}"))
                elif not result.ok:
                    self._ui_queue.put(
                        UiEvent(
                            kind="exec_error",
                            message=f"0\x00{req_id}\x00{result.payload}",
                        )
                    )
            except Exception as exc:
                self._ui_queue.put(
                    UiEvent(
                        kind="exec_error",
                        message=f"{pid}\x00{req_id}\x00EXEC failed: {exc}",
                    )
                )

        self._executor.submit(task)

    def send_once_with_retry(
        self,
        verb: str,
        payload: str,
        read_timeout_s: float | None = None,
        inactivity_timeout_s: float | None = None,
    ) -> ControlResponse:
        last_exc: Exception | None = None
        timeout = (
            read_timeout_s
            if read_timeout_s is not None
            else self._session.default_read_timeout_s
        )
        inactivity = (
            inactivity_timeout_s
            if inactivity_timeout_s is not None
            else self._session.default_inactivity_timeout_s
        )
        for attempt in range(1, self._session.request_retries + 1):
            try:
                return self._client.send_once(
                    verb=verb,
                    payload=payload,
                    agent_id="1",
                    read_timeout_s=timeout,
                    inactivity_timeout_s=inactivity,
                )
            except Exception as exc:
                last_exc = exc
                if attempt < self._session.request_retries:
                    time.sleep(self._session.retry_delay_s)

        raise RuntimeError(
            f"{verb} failed after {self._session.request_retries} attempts: {last_exc}"
        )

    def exec_stream_with_retry(self, prompt: str, on_frame) -> ControlResponse:
        last_exc: Exception | None = None
        for attempt in range(1, self._session.request_retries + 1):
            try:
                return self._client.exec_stream(
                    prompt=prompt,
                    agent_id="1",
                    on_frame=on_frame,
                    inactivity_timeout_s=60.0,
                    max_total_s=300.0,
                )
            except Exception as exc:
                last_exc = exc
                if attempt < self._session.request_retries:
                    time.sleep(self._session.retry_delay_s)

        raise RuntimeError(
            f"EXEC stream failed after {self._session.request_retries} attempts: {last_exc}"
        )

    @staticmethod
    def format_control(verb: str, response: ControlResponse) -> str:
        status = "OK" if response.ok else "ERR"
        return (
            f"[{verb}] status={status} code={response.code} "
            f"duration={response.duration_s:.3f}s\n{response.payload}\n"
        )
