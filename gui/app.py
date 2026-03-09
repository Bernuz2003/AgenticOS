from __future__ import annotations

import queue
import sys
from pathlib import Path

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
from gui.protocol_client import ProtocolClient
from gui.response_parser import (
    parse_json_dict,
    parse_pid_status_payload,
    parse_restore_payload,
    parse_status_payload,
)
from gui.sections import (
    ChatSection,
    LogsSection,
    MemorySection,
    ModelsSection,
    OrchestrationSection,
    ProcessesSection,
    SidebarWidget,
)
from gui.services import GuiSessionState, RequestHandler, UiEvent


class MainWindow(QMainWindow):
    """Main window: Sidebar (fixed 210px) + QStackedWidget with 6 sections."""

    def __init__(self, workspace_root: Path):
        super().__init__()
        self.workspace_root = workspace_root
        self.client = ProtocolClient()
        self.kernel = KernelProcessManager(workspace_root=workspace_root)
        self.session = GuiSessionState()
        self.ui_queue: queue.Queue[UiEvent] = queue.Queue()
        self.request_handler = RequestHandler(
            client=self.client,
            ui_queue=self.ui_queue,
            session=self.session,
            update_client_config=self._update_client_config,
            kernel_is_running=self.kernel.is_running,
        )

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
        self.models_section.backend_diag_requested.connect(self._request_backend_diag)
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
                online=self.session.is_connected,
                model_name=self.session.loaded_model_id or "—",
                proc_count=len(self.session.active_pids),
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
        self.session.is_connected = False
        self.session.last_status_payload = ""
        self.session.loaded_model_id = ""
        self.session.active_pids = []
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
        on_done=None,
    ):
        self.request_handler.dispatch_control_request(
            verb=verb,
            payload=payload,
            show_error_popup=show_error_popup,
            success_event_kind=success_event_kind,
            read_timeout_s=read_timeout_s,
            inactivity_timeout_s=inactivity_timeout_s,
            include_control_log=include_control_log,
            on_done=on_done,
        )

    # ── STATUS ───────────────────────────────────────────────

    def _refresh_status(self, force: bool = False):
        self.request_handler.refresh_status(force=force)

    # ── LIST_MODELS ──────────────────────────────────────────

    def _refresh_models(self, silent: bool = False, force: bool = False):
        self.request_handler.refresh_models(silent=silent, force=force)

    # ── LOAD (SELECT + LOAD) ────────────────────────────────

    def _load_model(self, model_id: str):
        self.request_handler.load_model(model_id, self.models_section.set_loading)

    def _request_backend_diag(self):
        self.request_handler.request_backend_diag()

    # ── EXEC ─────────────────────────────────────────────────

    def _exec_stream(self, prompt: str, workload: str):
        self.request_handler.exec_stream(
            prompt=prompt,
            workload=workload,
            last_status_payload=self.session.last_status_payload,
            begin_user_message=self.chat_section.begin_user_message,
            show_error=self.chat_section.show_error,
        )

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
                self.session.is_connected = False
                QMessageBox.critical(self, "AgenticOS", event.message)

            elif event.kind == "background_error":
                self.session.is_connected = False
                if event.message != self.session.last_background_error:
                    self.session.last_background_error = event.message
                self.sidebar.update_status(online=False)

            elif event.kind == "status":
                self.session.is_connected = True
                self.session.last_background_error = ""
                self.session.last_status_payload = event.message
                self._apply_status(event.message)

            elif event.kind == "models_list":
                self.models_section.update_models(event.message)

            elif event.kind == "model_info":
                self.models_section.show_info(event.message)

            elif event.kind == "backend_diag":
                self.models_section.show_backend_diag(event.message)

            elif event.kind == "backend_diag_error":
                self.models_section.show_backend_diag(event.message)

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
                self.processes_section.show_quota(parse_pid_status_payload(event.message))

            elif event.kind == "pid_status":
                detail = parse_pid_status_payload(event.message)
                if detail is not None:
                    self.processes_section.update_pid_detail(detail)

            elif event.kind == "checkpoint_done":
                self.memory_section.show_checkpoint_result(event.message)

            elif event.kind == "restore_done":
                restore = parse_restore_payload(event.message)
                if restore is None:
                    self.memory_section.show_restore_result(event.message)
                else:
                    self.memory_section.show_restore_details(restore)

            elif event.kind == "memw_done":
                self.memory_section.show_memw_result(event.message)

            elif event.kind == "orch_submit":
                self.orchestration_section.show_submit_result(parse_json_dict(event.message))

            elif event.kind == "orch_status":
                self.orchestration_section.update_orch_status(parse_json_dict(event.message))

            elif event.kind == "protocol_trace":
                parts = event.message.split("\x00", 2)
                if len(parts) == 3:
                    self.logs_section.append_protocol_trace(parts[0], parts[1], parts[2])

            elif event.kind == "control":
                pass  # Control log — could route to a debug panel in future

    def _apply_status(self, payload: str):
        """Parse JSON STATUS payload and update sidebar + models section."""
        status = parse_status_payload(payload)
        if status is None:
            return

        self.session.active_pids = status.active_pids

        # Loaded model
        if status.loaded_model_id:
            self.session.loaded_model_id = status.loaded_model_id
        else:
            self.session.loaded_model_id = ""

        self.models_section.update_loaded_model(self.session.loaded_model_id)

        # Pretty uptime
        secs = status.uptime_secs
        if secs >= 3600:
            uptime_str = f"{secs / 3600:.1f}h"
        elif secs >= 60:
            uptime_str = f"{secs / 60:.0f}m"
        else:
            uptime_str = f"{secs:.0f}s"

        # Pretty model name
        model_display = (
            self.session.loaded_model_id.split("/")[-1]
            if self.session.loaded_model_id
            else "—"
        )

        self.sidebar.update_status(
            online=True,
            model_name=model_display,
            proc_count=len(self.session.active_pids),
            uptime=uptime_str,
        )

        # Update chat PID input autocomplete hint
        if self.session.active_pids:
            self.chat_section.pid_input.setPlaceholderText(
                f"PIDs: {', '.join(self.session.active_pids[:3])}"
            )

        # Feed processes and memory sections (pass parsed dict)
        self.processes_section.update_from_status(status)
        self.memory_section.update_from_status(status)

    # ═══════════════════════════════════════════════════════════
    #  CLOSE EVENT
    # ═══════════════════════════════════════════════════════════

    def closeEvent(self, event):
        # Stop all timers
        for timer in (self._queue_timer, self._status_timer, self._events_timer,
                      self._audit_timer, self._models_timer):
            timer.stop()
        self.request_handler.shutdown()
        # Close persistent TCP connection
        self.client.close()
        super().closeEvent(event)

    def _drain_kernel_events(self):
        while True:
            try:
                event = self.kernel.events.get_nowait()
            except queue.Empty:
                break
            self.logs_section.append_kernel_event(event.source, event.line)


def main():
    root = Path(__file__).resolve().parents[1]
    app = QApplication(sys.argv)
    window = MainWindow(workspace_root=root)
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
