from __future__ import annotations

import json
import queue
import re
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from PySide6.QtCore import QTimer, Qt
from PySide6.QtWidgets import (
    QApplication,
    QHBoxLayout,
    QMainWindow,
    QMessageBox,
    QStackedWidget,
    QWidget,
)

from gui.kernel_manager import KernelProcessManager
from gui.protocol_client import ControlResponse, ProtocolClient
from gui.widgets.chat import ChatSection
from gui.widgets.logs import LogsSection
from gui.widgets.memory import MemorySection
from gui.widgets.models import ModelsSection
from gui.widgets.orchestration import OrchestrationSection
from gui.widgets.processes import ProcessesSection
from gui.widgets.sidebar import SidebarWidget


@dataclass
class UiEvent:
    kind: str
    message: str


class MainWindow(QMainWindow):
    """Main window: Sidebar (fixed 210px) + QStackedWidget with 6 sections."""

    def __init__(self, workspace_root: Path):
        super().__init__()
        self.workspace_root = workspace_root
        self.client = ProtocolClient()
        self.kernel = KernelProcessManager(workspace_root=workspace_root)
        self.ui_queue: queue.Queue[UiEvent] = queue.Queue()
        self._executor = ThreadPoolExecutor(max_workers=4)

        # Connection / retry config
        self._request_retries = 2
        self._retry_delay_s = 0.35
        self._is_connected = False
        self._last_background_error = ""
        self._status_in_flight = False
        self._models_in_flight = False
        self._load_in_flight = False
        self._active_pids: list[str] = []
        self._loaded_model_id = ""
        self._last_status_payload = ""

        self._default_read_timeout_s = 5.0
        self._default_inactivity_timeout_s = 0.5
        self._load_read_timeout_s = 180.0
        self._load_inactivity_timeout_s = 2.0

        self.setWindowTitle("AgenticOS Control Center")
        self.resize(1280, 820)

        self._build_ui()
        self._connect_signals()
        self._load_theme()
        self._setup_timers()

    # ═══════════════════════════════════════════════════════════
    #  UI CONSTRUCTION
    # ═══════════════════════════════════════════════════════════

    def _build_ui(self):
        root = QWidget()
        root_layout = QHBoxLayout(root)
        root_layout.setContentsMargins(0, 0, 0, 0)
        root_layout.setSpacing(0)

        # Sidebar
        self.sidebar = SidebarWidget()
        root_layout.addWidget(self.sidebar)

        # Stacked widget: index matches sidebar SECTIONS order
        self.stack = QStackedWidget()
        self.chat_section = ChatSection()
        self.models_section = ModelsSection()
        self.processes_section = ProcessesSection()
        self.memory_section = MemorySection()
        self.orchestration_section = OrchestrationSection()
        self.logs_section = LogsSection(workspace_root=self.workspace_root)

        self.stack.addWidget(self.chat_section)       # 0
        self.stack.addWidget(self.models_section)      # 1
        self.stack.addWidget(self.processes_section)    # 2
        self.stack.addWidget(self.memory_section)       # 3
        self.stack.addWidget(self.orchestration_section)  # 4
        self.stack.addWidget(self.logs_section)         # 5

        root_layout.addWidget(self.stack, stretch=1)
        self.setCentralWidget(root)

    def _connect_signals(self):
        # Sidebar navigation
        self.sidebar.section_changed.connect(self.stack.setCurrentIndex)
        self.sidebar.start_kernel.connect(self._start_kernel)
        self.sidebar.stop_kernel.connect(self._stop_kernel)

        # Chat section
        self.chat_section.exec_requested.connect(self._exec_stream)
        self.chat_section.term_requested.connect(lambda pid: self._send_pid_signal("TERM", pid))
        self.chat_section.kill_requested.connect(lambda pid: self._send_pid_signal("KILL", pid))
        self.chat_section.set_gen_requested.connect(
            lambda payload: self._send_simple("SET_GEN", payload)
        )
        self.chat_section.get_gen_requested.connect(
            lambda: self._send_with_event("GET_GEN", "", "gen_get")
        )

        # Models section
        self.models_section.load_requested.connect(self._load_model)
        self.models_section.select_requested.connect(
            lambda mid: self._send_simple("SELECT_MODEL", mid)
        )
        self.models_section.info_requested.connect(
            lambda mid: self._send_with_event("MODEL_INFO", mid, "model_info")
        )
        self.models_section.refresh_requested.connect(
            lambda: self._refresh_models(silent=False, force=True)
        )

        # Processes section
        self.processes_section.set_priority_requested.connect(
            lambda pid, lvl: self._send_simple("SET_PRIORITY", f"{pid} {lvl}")
        )
        self.processes_section.set_quota_requested.connect(
            lambda pid, payload: self._send_simple("SET_QUOTA", f"{pid} {payload}")
        )
        self.processes_section.get_quota_requested.connect(
            lambda pid: self._send_with_event("GET_QUOTA", pid, "quota_response")
        )
        self.processes_section.term_requested.connect(
            lambda pid: self._send_pid_signal("TERM", pid)
        )
        self.processes_section.kill_requested.connect(
            lambda pid: self._send_pid_signal("KILL", pid)
        )
        self.processes_section.status_pid_requested.connect(
            lambda pid: self._send_with_event("STATUS", pid, "pid_status")
        )
        self.processes_section.refresh_requested.connect(
            lambda: self._refresh_status(force=True)
        )

        # Memory section
        self.memory_section.checkpoint_requested.connect(
            lambda path: self._send_with_event("CHECKPOINT", path, "checkpoint_done")
        )
        self.memory_section.restore_requested.connect(
            lambda path: self._send_with_event("RESTORE", path, "restore_done")
        )
        self.memory_section.memw_requested.connect(
            lambda pid, data: self._send_with_event("MEMW", f"{pid}\n{data}", "memw_done")
        )
        self.memory_section.refresh_requested.connect(
            lambda: self._refresh_status(force=True)
        )

        # Orchestration section
        self.orchestration_section.orchestrate_requested.connect(
            lambda payload: self._send_with_event("ORCHESTRATE", payload, "orch_submit")
        )
        self.orchestration_section.poll_orch_requested.connect(
            lambda oid: self._send_with_event("STATUS", f"orch:{oid}", "orch_status")
        )
        self.orchestration_section.refresh_requested.connect(
            lambda: self._refresh_status(force=True)
        )

        # Logs section
        self.logs_section.export_requested.connect(
            lambda: self.sidebar.update_status(
                online=self._is_connected,
                model_name=self._loaded_model_id or "—",
                proc_count=len(self._active_pids),
            )
        )

    def _load_theme(self):
        qss_path = Path(__file__).parent / "styles" / "theme.qss"
        if qss_path.exists():
            self.setStyleSheet(qss_path.read_text(encoding="utf-8"))

    # ═══════════════════════════════════════════════════════════
    #  TIMERS
    # ═══════════════════════════════════════════════════════════

    def _setup_timers(self):
        self._queue_timer = QTimer(self)
        self._queue_timer.timeout.connect(self._flush_ui_queue)
        self._queue_timer.start(100)

        self._status_timer = QTimer(self)
        self._status_timer.timeout.connect(self._refresh_status)
        self._status_timer.start(5000)

        self._events_timer = QTimer(self)
        self._events_timer.timeout.connect(self._drain_kernel_events)
        self._events_timer.start(200)

        self._audit_timer = QTimer(self)
        self._audit_timer.timeout.connect(self.logs_section.poll_syscall_audit)
        self._audit_timer.start(500)

        self._models_timer = QTimer(self)
        self._models_timer.timeout.connect(lambda: self._refresh_models(silent=True, force=False))
        self._models_timer.start(15000)

    # ═══════════════════════════════════════════════════════════
    #  KERNEL LIFECYCLE
    # ═══════════════════════════════════════════════════════════

    def _start_kernel(self):
        self.client.reset_session()
        ok, msg = self.kernel.start()
        if not ok:
            QMessageBox.critical(self, "AgenticOS", msg)
            return
        self.client.reset_session()
        QTimer.singleShot(900, lambda: self._refresh_status(force=True))
        QTimer.singleShot(1200, lambda: self._refresh_models(silent=True, force=True))

    def _stop_kernel(self):
        self.client.reset_session()
        ok, msg = self.kernel.stop()
        if not ok:
            QMessageBox.critical(self, "AgenticOS", msg)
        self.client.reset_session()
        self._is_connected = False
        self._last_status_payload = ""
        self._loaded_model_id = ""
        self._active_pids = []
        self.models_section.update_loaded_model("")
        self.sidebar.update_status(online=False)

    # ═══════════════════════════════════════════════════════════
    #  PROTOCOL DISPATCH (background threads → ui_queue)
    # ═══════════════════════════════════════════════════════════

    def _update_client_config(self):
        pass  # client configured once in __init__

    def _send_simple(self, verb: str, payload: str, show_error_popup: bool = True):
        self._dispatch_control_request(verb=verb, payload=payload, show_error_popup=show_error_popup)

    def _send_with_event(self, verb: str, payload: str, event_kind: str, show_error_popup: bool = True):
        self._dispatch_control_request(
            verb=verb, payload=payload,
            show_error_popup=show_error_popup,
            success_event_kind=event_kind,
        )

    def _dispatch_control_request(
        self,
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
                response = self._send_once_with_retry(
                    verb=verb, payload=payload,
                    read_timeout_s=read_timeout_s,
                    inactivity_timeout_s=inactivity_timeout_s,
                )
                if not response.ok:
                    message = f"{verb} failed [{response.code}]: {response.payload}"
                    kind = "error" if show_error_popup else "background_error"
                    self.ui_queue.put(UiEvent(kind=kind, message=message))
                elif success_event_kind:
                    self.ui_queue.put(UiEvent(kind=success_event_kind, message=response.payload))
                if include_control_log:
                    self.ui_queue.put(UiEvent(kind="control", message=self._format_control(verb, response)))
                # Protocol trace — routed through queue for thread safety
                self.ui_queue.put(UiEvent(kind="protocol_trace", message=f"{verb}\x00{payload}\x00{response.payload}"))
            except Exception as exc:
                kind = "error" if show_error_popup else "background_error"
                self.ui_queue.put(UiEvent(kind=kind, message=f"{verb} failed: {exc}"))
            finally:
                if on_done is not None:
                    try:
                        on_done()
                    except Exception:
                        pass

        self._executor.submit(task)

    # ── STATUS ───────────────────────────────────────────────

    def _refresh_status(self, force: bool = False):
        if self._status_in_flight:
            return
        if self._load_in_flight and not force:
            return
        should_poll = force or self._is_connected or self.kernel.is_running()
        if not should_poll:
            return

        self._status_in_flight = True

        def task():
            try:
                self._update_client_config()
                response = self._send_once_with_retry(
                    verb="STATUS", payload="",
                    read_timeout_s=6.0, inactivity_timeout_s=0.35,
                )
                if response.ok:
                    self.ui_queue.put(UiEvent(kind="status", message=response.payload))
                else:
                    self.ui_queue.put(
                        UiEvent(
                            kind="background_error",
                            message=f"STATUS failed [{response.code}]: {response.payload}",
                        )
                    )
            except Exception as exc:
                self.ui_queue.put(UiEvent(kind="background_error", message=f"STATUS failed: {exc}"))
            finally:
                self._status_in_flight = False

        self._executor.submit(task)

    # ── LIST_MODELS ──────────────────────────────────────────

    def _refresh_models(self, silent: bool = False, force: bool = False):
        if self._models_in_flight:
            return
        should_refresh = force or self._is_connected or self.kernel.is_running() or not silent
        if not should_refresh:
            return

        self._models_in_flight = True
        self._dispatch_control_request(
            verb="LIST_MODELS", payload="",
            show_error_popup=not silent,
            success_event_kind="models_list",
            read_timeout_s=8.0, inactivity_timeout_s=0.6,
            include_control_log=not silent,
            on_done=lambda: setattr(self, "_models_in_flight", False),
        )

    # ── LOAD (SELECT + LOAD) ────────────────────────────────

    def _load_model(self, model_id: str):
        if self._load_in_flight:
            return
        if not model_id:
            return

        self._load_in_flight = True
        self.models_section.set_loading(model_id)

        def task():
            try:
                self._update_client_config()
                select_response = self._send_once_with_retry(
                    "SELECT_MODEL", model_id,
                    read_timeout_s=8.0, inactivity_timeout_s=0.6,
                )
                if not select_response.ok:
                    raise RuntimeError(
                        f"SELECT_MODEL failed [{select_response.code}]: {select_response.payload}"
                    )
                load_response = self._send_once_with_retry(
                    "LOAD", "",
                    read_timeout_s=self._load_read_timeout_s,
                    inactivity_timeout_s=self._load_inactivity_timeout_s,
                )
                if not load_response.ok:
                    raise RuntimeError(f"LOAD failed [{load_response.code}]: {load_response.payload}")
                self.ui_queue.put(UiEvent(kind="refresh_after_load", message=model_id))
            except Exception as exc:
                self.ui_queue.put(UiEvent(kind="error", message=f"LOAD failed: {exc}"))
            finally:
                self._load_in_flight = False

        self._executor.submit(task)

    # ── EXEC ─────────────────────────────────────────────────

    def _exec_stream(self, prompt: str, workload: str):
        if not prompt:
            return
        if "no_model_loaded=true" in self._last_status_payload.lower():
            self.chat_section.show_error("Nessun modello caricato — usa Models → Load prima di Chat.")
            return

        outbound_prompt = prompt
        if workload and workload != "auto":
            outbound_prompt = f"capability={workload}; {prompt}"

        req_id = self.chat_section.begin_user_message(prompt)

        def task():
            pid = 0
            try:
                self._update_client_config()

                def on_frame(kind: str, code: str, body: bytes):
                    nonlocal pid
                    text = body.decode("utf-8", errors="replace")
                    if kind == "+OK":
                        m = re.search(r"PID:\s*(\d+)", text)
                        if m:
                            pid = int(m.group(1))
                            self.ui_queue.put(UiEvent(
                                kind="exec_started",
                                message=f"{pid}\x00{req_id}",
                            ))
                    elif kind == "DATA" and code.lower() == "raw":
                        finish_match = re.search(
                            r"\[PROCESS_FINISHED\s+pid=(\d+)\s+tokens_generated=(\d+)\s+elapsed_secs=([0-9.]+)\]",
                            text,
                        )
                        if finish_match:
                            self.ui_queue.put(UiEvent(
                                kind="exec_metrics",
                                message=(
                                    f"{finish_match.group(1)}\x00"
                                    f"{finish_match.group(2)}\x00"
                                    f"{finish_match.group(3)}"
                                ),
                            ))
                        self.ui_queue.put(UiEvent(
                            kind="exec_stream",
                            message=f"{pid}\x00{text}",
                        ))

                result = self._exec_stream_with_retry(prompt=outbound_prompt, on_frame=on_frame)
                if pid > 0:
                    self.ui_queue.put(UiEvent(
                        kind="exec_done", message=f"{pid}\x00{req_id}",
                    ))
                elif not result.ok:
                    self.ui_queue.put(UiEvent(
                        kind="exec_error",
                        message=f"0\x00{req_id}\x00{result.payload}",
                    ))
            except Exception as exc:
                self.ui_queue.put(UiEvent(
                    kind="exec_error",
                    message=f"{pid}\x00{req_id}\x00EXEC failed: {exc}",
                ))

        threading.Thread(target=task, daemon=True).start()

    # ── PID signals ──────────────────────────────────────────

    def _send_pid_signal(self, verb: str, pid: str):
        if not pid:
            return
        self._send_simple(verb, pid)

    # ═══════════════════════════════════════════════════════════
    #  UI QUEUE FLUSH → route events to widgets
    # ═══════════════════════════════════════════════════════════

    def _flush_ui_queue(self):
        while True:
            try:
                event = self.ui_queue.get_nowait()
            except queue.Empty:
                break

            if event.kind == "error":
                self._is_connected = False
                QMessageBox.critical(self, "AgenticOS", event.message)

            elif event.kind == "background_error":
                self._is_connected = False
                if event.message != self._last_background_error:
                    self._last_background_error = event.message
                self.sidebar.update_status(online=False)

            elif event.kind == "status":
                self._is_connected = True
                self._last_background_error = ""
                self._last_status_payload = event.message
                self._apply_status(event.message)

            elif event.kind == "models_list":
                self.models_section.update_models(event.message)

            elif event.kind == "model_info":
                self.models_section.show_info(event.message)

            elif event.kind == "refresh_after_load":
                self._refresh_status(force=True)
                self._refresh_models(silent=True, force=True)

            elif event.kind == "gen_get":
                self.chat_section.apply_generation(event.message)

            elif event.kind == "exec_started":
                parts = event.message.split("\x00", 1)
                self.chat_section.start_assistant_stream(int(parts[0]), int(parts[1]))

            elif event.kind == "exec_stream":
                pid_str, text = event.message.split("\x00", 1)
                self.chat_section.append_assistant_chunk(int(pid_str), text)

            elif event.kind == "exec_done":
                parts = event.message.split("\x00", 1)
                self.chat_section.finish_assistant_message(int(parts[0]))

            elif event.kind == "exec_metrics":
                pid_str, tok_str, elapsed_str = event.message.split("\x00", 2)
                self.chat_section.apply_process_metrics(
                    int(pid_str),
                    int(tok_str),
                    float(elapsed_str),
                )

            elif event.kind == "exec_error":
                parts = event.message.split("\x00", 2)
                pid = int(parts[0]) if len(parts) > 0 else 0
                req_id = int(parts[1]) if len(parts) > 1 else 0
                msg = parts[2] if len(parts) > 2 else event.message
                self.chat_section.show_error(msg, pid=pid, req_id=req_id)

            elif event.kind == "quota_response":
                try:
                    quota_data = json.loads(event.message)
                except (json.JSONDecodeError, TypeError):
                    quota_data = {}
                self.processes_section.show_quota(quota_data)

            elif event.kind == "pid_status":
                try:
                    pid_data = json.loads(event.message)
                    pid = str(pid_data.get("pid", ""))
                except (json.JSONDecodeError, TypeError):
                    pid = ""
                    pid_data = {}
                self.processes_section.update_pid_detail(pid, pid_data)

            elif event.kind == "checkpoint_done":
                self.memory_section.show_checkpoint_result(event.message)

            elif event.kind == "restore_done":
                self.memory_section.show_restore_result(event.message)

            elif event.kind == "memw_done":
                self.memory_section.show_memw_result(event.message)

            elif event.kind == "orch_submit":
                try:
                    orch_data = json.loads(event.message)
                except (json.JSONDecodeError, TypeError):
                    orch_data = {}
                self.orchestration_section.show_submit_result(orch_data)

            elif event.kind == "orch_status":
                try:
                    orch_data = json.loads(event.message)
                except (json.JSONDecodeError, TypeError):
                    orch_data = {}
                self.orchestration_section.update_orch_status(orch_data)

            elif event.kind == "protocol_trace":
                parts = event.message.split("\x00", 2)
                if len(parts) == 3:
                    self.logs_section.append_protocol_trace(parts[0], parts[1], parts[2])

            elif event.kind == "control":
                pass  # Control log — could route to a debug panel in future

    def _apply_status(self, payload: str):
        """Parse JSON STATUS payload and update sidebar + models section."""
        try:
            data = json.loads(payload)
        except (json.JSONDecodeError, TypeError):
            return

        model = data.get("model", {})
        loaded = model.get("loaded_model_id", "")
        procs = data.get("processes", {})
        active_pids = procs.get("active_pids", [])
        in_flight_pids = procs.get("in_flight_pids", [])

        self._active_pids = [str(p) for p in active_pids] + [str(p) for p in in_flight_pids]

        # Loaded model
        if loaded:
            self._loaded_model_id = loaded
        else:
            self._loaded_model_id = ""

        self.models_section.update_loaded_model(self._loaded_model_id)

        # Pretty uptime
        secs = data.get("uptime_secs", 0)
        if secs >= 3600:
            uptime_str = f"{secs / 3600:.1f}h"
        elif secs >= 60:
            uptime_str = f"{secs / 60:.0f}m"
        else:
            uptime_str = f"{secs:.0f}s"

        # Pretty model name
        model_display = self._loaded_model_id.split("/")[-1] if self._loaded_model_id else "—"

        self.sidebar.update_status(
            online=True,
            model_name=model_display,
            proc_count=len(self._active_pids),
            uptime=uptime_str,
        )

        # Update chat PID input autocomplete hint
        if self._active_pids:
            self.chat_section.pid_input.setPlaceholderText(
                f"PIDs: {', '.join(self._active_pids[:3])}"
            )

        # Feed processes and memory sections (pass parsed dict)
        self.processes_section.update_from_status(data)
        self.memory_section.update_from_status(data)

    # ═══════════════════════════════════════════════════════════
    #  CLOSE EVENT
    # ═══════════════════════════════════════════════════════════

    def closeEvent(self, event):
        # Stop all timers
        for timer in (self._queue_timer, self._status_timer, self._events_timer,
                      self._audit_timer, self._models_timer):
            timer.stop()
        # Shutdown thread pool (don't wait — daemon semantics)
        self._executor.shutdown(wait=False, cancel_futures=True)
        # Close persistent TCP connection
        self.client.close()
        super().closeEvent(event)

    # ═══════════════════════════════════════════════════════════
    #  KERNEL EVENTS → Logs section
    # ═══════════════════════════════════════════════════════════

    def _drain_kernel_events(self):
        while True:
            try:
                event = self.kernel.events.get_nowait()
            except queue.Empty:
                break
            self.logs_section.append_kernel_event(event.source, event.line)

    # ═══════════════════════════════════════════════════════════
    #  RETRY HELPERS
    # ═══════════════════════════════════════════════════════════

    def _send_once_with_retry(
        self,
        verb: str,
        payload: str,
        read_timeout_s: float | None = None,
        inactivity_timeout_s: float | None = None,
    ) -> ControlResponse:
        last_exc: Exception | None = None
        timeout = read_timeout_s if read_timeout_s is not None else self._default_read_timeout_s
        inactivity = (
            inactivity_timeout_s
            if inactivity_timeout_s is not None
            else self._default_inactivity_timeout_s
        )
        for attempt in range(1, self._request_retries + 1):
            try:
                return self.client.send_once(
                    verb=verb, payload=payload, agent_id="1",
                    read_timeout_s=timeout, inactivity_timeout_s=inactivity,
                )
            except Exception as exc:
                last_exc = exc
                if attempt < self._request_retries:
                    time.sleep(self._retry_delay_s)

        raise RuntimeError(f"{verb} failed after {self._request_retries} attempts: {last_exc}")

    def _exec_stream_with_retry(self, prompt: str, on_frame) -> ControlResponse:
        last_exc: Exception | None = None
        for attempt in range(1, self._request_retries + 1):
            try:
                return self.client.exec_stream(
                    prompt=prompt, agent_id="1", on_frame=on_frame,
                    inactivity_timeout_s=60.0,
                    max_total_s=300.0,
                )
            except Exception as exc:
                last_exc = exc
                if attempt < self._request_retries:
                    time.sleep(self._retry_delay_s)

        raise RuntimeError(f"EXEC stream failed after {self._request_retries} attempts: {last_exc}")

    # ═══════════════════════════════════════════════════════════
    #  HELPERS
    # ═══════════════════════════════════════════════════════════

    @staticmethod
    def _format_control(verb: str, response: ControlResponse) -> str:
        status = "OK" if response.ok else "ERR"
        return f"[{verb}] status={status} code={response.code} duration={response.duration_s:.3f}s\n{response.payload}\n"


def main():
    root = Path(__file__).resolve().parents[1]
    app = QApplication(sys.argv)
    window = MainWindow(workspace_root=root)
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
