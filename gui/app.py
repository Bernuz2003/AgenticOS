from __future__ import annotations

import datetime
import queue
import re
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from PySide6.QtCore import QTimer, Qt
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QGridLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QPlainTextEdit,
    QSplitter,
    QTabWidget,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from gui.kernel_manager import KernelProcessManager
from gui.protocol_client import ControlResponse, ProtocolClient


@dataclass
class UiEvent:
    kind: str
    message: str


class MainWindow(QMainWindow):
    def __init__(self, workspace_root: Path):
        super().__init__()
        self.workspace_root = workspace_root
        self.client = ProtocolClient()
        self.kernel = KernelProcessManager(workspace_root=workspace_root)
        self.ui_queue: queue.Queue[UiEvent] = queue.Queue()
        self._request_retries = 2
        self._retry_delay_s = 0.35
        self._is_connected = False
        self._last_background_error = ""
        self._status_in_flight = False
        self._models_in_flight = False
        self._load_in_flight = False
        self._active_pids: list[str] = []
        self._default_read_timeout_s = 5.0
        self._default_inactivity_timeout_s = 0.5
        self._load_read_timeout_s = 180.0
        self._load_inactivity_timeout_s = 2.0

        self._audit_offset = 0
        self._kernel_event_lines: list[str] = []
        self._syscall_event_lines: list[str] = []
        self._max_log_lines = 4000

        self.setWindowTitle("AgenticOS Control Center")
        self.resize(1280, 820)

        self.host_input = QLineEdit("127.0.0.1")
        self.port_input = QLineEdit("6379")
        self.agent_input = QLineEdit("1")
        self.status_label = QLabel("Disconnected")

        self.exec_prompt = QTextEdit()
        self.exec_output = QPlainTextEdit()
        self.exec_output.setReadOnly(True)
        self.pid_input = QLineEdit()
        self.pid_input.setPlaceholderText("PID da active_pids (es: 42)")
        self.pid_input.setFixedWidth(170)
        self.pid_combo = QComboBox()
        self.pid_combo.setMinimumWidth(180)
        self.pid_combo.setEditable(False)
        self.pid_combo.addItem("active_pids: none", "")

        self.command_input = QLineEdit("STATUS")
        self.command_output = QPlainTextEdit()
        self.command_output.setReadOnly(True)
        self.model_combo = QComboBox()
        self.model_combo.setMinimumWidth(420)
        self.model_combo.setEditable(False)
        self.selected_model_value = QLabel("<none>")
        self.loaded_model_value = QLabel("<none>")

        self.gen_temp = QDoubleSpinBox()
        self.gen_top_p = QDoubleSpinBox()
        self.gen_seed = QLineEdit("42")
        self.gen_max_tokens = QLineEdit("512")

        self.gen_temp.setRange(0.0, 2.0)
        self.gen_temp.setSingleStep(0.05)
        self.gen_temp.setValue(0.6)
        self.gen_top_p.setRange(0.0, 1.0)
        self.gen_top_p.setSingleStep(0.05)
        self.gen_top_p.setValue(0.9)

        self.kernel_events = QPlainTextEdit()
        self.kernel_events.setReadOnly(True)
        self.syscall_log = QPlainTextEdit()
        self.syscall_log.setReadOnly(True)
        self.kernel_filter_input = QLineEdit()
        self.syscall_filter_input = QLineEdit()
        self.show_stdout_cb = QCheckBox("stdout")
        self.show_stderr_cb = QCheckBox("stderr")
        self.show_noise_cb = QCheckBox("verbose kernel")
        self.show_stdout_cb.setChecked(True)
        self.show_stderr_cb.setChecked(True)
        self.show_noise_cb.setChecked(False)
        self.export_snapshot_btn = QPushButton("Export snapshot")

        self.status_snapshot = QPlainTextEdit()
        self.status_snapshot.setReadOnly(True)

        self.refresh_models_btn: QPushButton | None = None
        self.model_info_btn: QPushButton | None = None
        self.select_model_btn: QPushButton | None = None
        self.load_selected_btn: QPushButton | None = None
        self.exec_stream_btn: QPushButton | None = None

        self._build_ui()
        self._setup_timers()

    def _build_ui(self):
        root = QWidget()
        root_layout = QVBoxLayout(root)
        root_layout.addWidget(self._build_connection_panel())
        root_layout.addWidget(self._build_tabs())
        self.setCentralWidget(root)

    def _build_connection_panel(self) -> QWidget:
        box = QGroupBox("Kernel Session")
        layout = QGridLayout(box)

        layout.addWidget(QLabel("Host"), 0, 0)
        layout.addWidget(self.host_input, 0, 1)
        layout.addWidget(QLabel("Port"), 0, 2)
        layout.addWidget(self.port_input, 0, 3)
        layout.addWidget(QLabel("Agent"), 0, 4)
        layout.addWidget(self.agent_input, 0, 5)

        start_btn = QPushButton("Start Kernel")
        stop_btn = QPushButton("Stop Local Kernel")
        ping_btn = QPushButton("PING")
        status_btn = QPushButton("Refresh Runtime Status")

        stop_btn.setToolTip("Ferma il processo kernel avviato dalla GUI (livello OS).")
        status_btn.setToolTip("Richiede uno snapshot runtime: uptime, pids, errori, memoria, stato modello.")

        start_btn.clicked.connect(self._start_kernel)
        stop_btn.clicked.connect(self._stop_kernel)
        ping_btn.clicked.connect(lambda: self._send_simple("PING", ""))
        status_btn.clicked.connect(lambda: self._refresh_status(force=True))

        layout.addWidget(start_btn, 1, 0, 1, 2)
        layout.addWidget(stop_btn, 1, 2, 1, 2)
        layout.addWidget(ping_btn, 1, 4)
        layout.addWidget(status_btn, 1, 5)

        layout.addWidget(QLabel("State"), 2, 0)
        layout.addWidget(self.status_label, 2, 1, 1, 5)

        return box

    def _build_tabs(self) -> QWidget:
        tabs = QTabWidget()
        tabs.addTab(self._build_exec_tab(), "Exec")
        tabs.addTab(self._build_control_tab(), "Control")
        tabs.addTab(self._build_observability_tab(), "Behind the scenes")
        return tabs

    def _build_exec_tab(self) -> QWidget:
        container = QWidget()
        layout = QVBoxLayout(container)

        self.exec_prompt.setPlaceholderText("Inserisci prompt EXEC...")
        layout.addWidget(self.exec_prompt)

        btn_row = QHBoxLayout()
        exec_btn = QPushButton("EXEC Stream")
        self.exec_stream_btn = exec_btn
        term_btn = QPushButton("Stop PID (TERM)")
        kill_btn = QPushButton("Kill PID (KILL)")
        use_pid_btn = QPushButton("Use selected PID")

        exec_btn.setToolTip("Esegue il prompt in streaming sul modello caricato.")
        term_btn.setToolTip("Interruzione gentile del processo agentico PID.")
        kill_btn.setToolTip("Interruzione forzata del processo agentico PID.")
        use_pid_btn.setToolTip("Copia nel campo PID il valore selezionato da active_pids.")

        exec_btn.clicked.connect(self._exec_stream)
        term_btn.clicked.connect(lambda: self._send_pid_signal("TERM"))
        kill_btn.clicked.connect(lambda: self._send_pid_signal("KILL"))
        use_pid_btn.clicked.connect(self._use_selected_pid)
        self.pid_combo.currentIndexChanged.connect(self._use_selected_pid)

        btn_row.addWidget(exec_btn)
        btn_row.addWidget(self.pid_input)
        btn_row.addWidget(self.pid_combo)
        btn_row.addWidget(use_pid_btn)
        btn_row.addWidget(term_btn)
        btn_row.addWidget(kill_btn)
        btn_row.addStretch()
        layout.addLayout(btn_row)

        layout.addWidget(self.exec_output)
        return container

    def _build_control_tab(self) -> QWidget:
        container = QWidget()
        layout = QVBoxLayout(container)

        model_box = QGroupBox("Models")
        model_layout = QVBoxLayout(model_box)

        model_btn_row = QHBoxLayout()
        refresh_models_btn = QPushButton("Refresh LIST_MODELS")
        model_info_btn = QPushButton("MODEL_INFO")
        select_btn = QPushButton("SELECT_MODEL")
        load_selected_btn = QPushButton("LOAD selected")
        self.refresh_models_btn = refresh_models_btn
        self.model_info_btn = model_info_btn
        self.select_model_btn = select_btn
        self.load_selected_btn = load_selected_btn
        model_btn_row.addWidget(refresh_models_btn)
        model_btn_row.addWidget(model_info_btn)
        model_btn_row.addWidget(select_btn)
        model_btn_row.addWidget(load_selected_btn)
        model_btn_row.addStretch()
        model_layout.addLayout(model_btn_row)

        model_form_row = QHBoxLayout()
        model_form_row.addWidget(QLabel("Model"))
        model_form_row.addWidget(self.model_combo)
        model_layout.addLayout(model_form_row)

        model_state_row = QGridLayout()
        model_state_row.addWidget(QLabel("Selected model"), 0, 0)
        model_state_row.addWidget(self.selected_model_value, 0, 1)
        model_state_row.addWidget(QLabel("Loaded model"), 1, 0)
        model_state_row.addWidget(self.loaded_model_value, 1, 1)
        model_layout.addLayout(model_state_row)

        refresh_models_btn.clicked.connect(lambda: self._refresh_models(silent=False, force=True))
        model_info_btn.clicked.connect(self._model_info)
        select_btn.clicked.connect(self._select_model)
        load_selected_btn.clicked.connect(self._load_selected_model)

        gen_box = QGroupBox("Generation")
        gen_layout = QGridLayout(gen_box)
        gen_layout.addWidget(QLabel("temperature"), 0, 0)
        gen_layout.addWidget(self.gen_temp, 0, 1)
        gen_layout.addWidget(QLabel("top_p"), 0, 2)
        gen_layout.addWidget(self.gen_top_p, 0, 3)
        gen_layout.addWidget(QLabel("seed"), 1, 0)
        gen_layout.addWidget(self.gen_seed, 1, 1)
        gen_layout.addWidget(QLabel("max_tokens"), 1, 2)
        gen_layout.addWidget(self.gen_max_tokens, 1, 3)

        get_gen_btn = QPushButton("GET_GEN")
        set_gen_btn = QPushButton("SET_GEN")
        gen_layout.addWidget(get_gen_btn, 2, 2)
        gen_layout.addWidget(set_gen_btn, 2, 3)
        get_gen_btn.clicked.connect(self._get_generation)
        set_gen_btn.clicked.connect(self._set_generation)

        quick_row = QHBoxLayout()
        shutdown_btn = QPushButton("Kernel SHUTDOWN (graceful)")
        shutdown_btn.setToolTip("Invia comando SHUTDOWN al kernel via protocollo.")
        shutdown_btn.clicked.connect(lambda: self._send_simple("SHUTDOWN", ""))
        quick_row.addWidget(shutdown_btn)
        quick_row.addStretch()

        custom_row = QHBoxLayout()
        self.command_input.setPlaceholderText("Comando custom (es: SELECT_MODEL model_id)")
        run_btn = QPushButton("Run")
        run_btn.clicked.connect(self._run_custom_command)
        custom_row.addWidget(self.command_input)
        custom_row.addWidget(run_btn)

        layout.addWidget(model_box)
        layout.addWidget(gen_box)
        layout.addLayout(quick_row)
        layout.addLayout(custom_row)

        splitter = QSplitter(Qt.Orientation.Vertical)
        splitter.addWidget(self.command_output)
        splitter.addWidget(self.status_snapshot)
        splitter.setSizes([350, 250])

        layout.addWidget(splitter)
        return container

    def _build_observability_tab(self) -> QWidget:
        container = QWidget()
        layout = QVBoxLayout(container)

        filter_row = QHBoxLayout()
        self.kernel_filter_input.setPlaceholderText("Filter kernel events...")
        self.syscall_filter_input.setPlaceholderText("Filter syscall audit...")
        filter_row.addWidget(QLabel("Kernel"))
        filter_row.addWidget(self.kernel_filter_input)
        filter_row.addWidget(self.show_stdout_cb)
        filter_row.addWidget(self.show_stderr_cb)
        filter_row.addWidget(self.show_noise_cb)
        filter_row.addSpacing(12)
        filter_row.addWidget(QLabel("Syscall"))
        filter_row.addWidget(self.syscall_filter_input)
        filter_row.addWidget(self.export_snapshot_btn)
        layout.addLayout(filter_row)

        self.kernel_filter_input.textChanged.connect(self._render_kernel_events)
        self.syscall_filter_input.textChanged.connect(self._render_syscall_events)
        self.show_stdout_cb.toggled.connect(self._render_kernel_events)
        self.show_stderr_cb.toggled.connect(self._render_kernel_events)
        self.show_noise_cb.toggled.connect(self._render_kernel_events)
        self.export_snapshot_btn.clicked.connect(self._export_snapshot)

        splitter = QSplitter(Qt.Orientation.Horizontal)
        splitter.addWidget(self.kernel_events)
        splitter.addWidget(self.syscall_log)
        splitter.setSizes([700, 500])

        self.kernel_events.setPlaceholderText("Eventi runtime da stdout/stderr kernel...")
        self.syscall_log.setPlaceholderText("Tail di workspace/syscall_audit.log...")

        layout.addWidget(splitter)
        return container

    def _setup_timers(self):
        self.queue_timer = QTimer(self)
        self.queue_timer.timeout.connect(self._flush_ui_queue)
        self.queue_timer.start(100)

        self.status_timer = QTimer(self)
        self.status_timer.timeout.connect(self._refresh_status)
        self.status_timer.start(5000)

        self.events_timer = QTimer(self)
        self.events_timer.timeout.connect(self._drain_kernel_events)
        self.events_timer.start(200)

        self.audit_timer = QTimer(self)
        self.audit_timer.timeout.connect(self._tail_syscall_audit)
        self.audit_timer.start(500)

        self.models_timer = QTimer(self)
        self.models_timer.timeout.connect(lambda: self._refresh_models(silent=True, force=False))
        self.models_timer.start(15000)

    def _update_client_config(self):
        host = self.host_input.text().strip() or "127.0.0.1"
        port_text = self.port_input.text().strip() or "6379"
        try:
            port = int(port_text)
        except ValueError:
            raise ValueError("Port non valido")

        self.client = ProtocolClient(host=host, port=port)

    def _set_status(self, text: str):
        self.status_label.setText(text)

    def _set_controls_enabled(self, enabled: bool):
        controls = [
            self.refresh_models_btn,
            self.model_info_btn,
            self.select_model_btn,
            self.load_selected_btn,
            self.exec_stream_btn,
        ]
        for control in controls:
            if control is not None:
                control.setEnabled(enabled)

    def _start_kernel(self):
        ok, msg = self.kernel.start()
        self._set_status(msg)
        if not ok:
            self._show_error(msg)
            return

        QTimer.singleShot(900, lambda: self._refresh_status(force=True))
        QTimer.singleShot(1200, lambda: self._refresh_models(silent=True, force=True))

    def _stop_kernel(self):
        ok, msg = self.kernel.stop()
        self._set_status(msg)
        if not ok:
            self._show_error(msg)

    def _send_simple(self, verb: str, payload: str, show_error_popup: bool = True):
        self._dispatch_control_request(
            verb=verb,
            payload=payload,
            show_error_popup=show_error_popup,
        )

    def _send_with_event(
        self,
        verb: str,
        payload: str,
        event_kind: str,
        show_error_popup: bool = True,
    ):
        self._dispatch_control_request(
            verb=verb,
            payload=payload,
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
                    verb=verb,
                    payload=payload,
                    read_timeout_s=read_timeout_s,
                    inactivity_timeout_s=inactivity_timeout_s,
                )
                if success_event_kind:
                    self.ui_queue.put(UiEvent(kind=success_event_kind, message=response.payload))
                if include_control_log:
                    self.ui_queue.put(
                        UiEvent(
                            kind="control",
                            message=self._format_control(verb, response),
                        )
                    )
            except Exception as exc:
                kind = "error" if show_error_popup else "background_error"
                self.ui_queue.put(UiEvent(kind=kind, message=f"{verb} failed: {exc}"))
            finally:
                if on_done is not None:
                    try:
                        on_done()
                    except Exception:
                        pass

        threading.Thread(target=task, daemon=True).start()

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
                    verb="STATUS",
                    payload="",
                    read_timeout_s=6.0,
                    inactivity_timeout_s=0.35,
                )
                self.ui_queue.put(UiEvent(kind="status", message=self._format_control("STATUS", response)))
            except Exception as exc:
                self.ui_queue.put(UiEvent(kind="background_error", message=f"STATUS failed: {exc}"))
            finally:
                self._status_in_flight = False

        threading.Thread(target=task, daemon=True).start()

    def _run_custom_command(self):
        raw = self.command_input.text().strip()
        if not raw:
            return

        parts = raw.split(" ", 1)
        verb = parts[0].upper()
        payload = parts[1] if len(parts) > 1 else ""
        self._send_simple(verb, payload)

    def _refresh_models(self, silent: bool = False, force: bool = False):
        if self._models_in_flight:
            return

        should_refresh = force or self._is_connected or self.kernel.is_running() or not silent
        if not should_refresh:
            return

        self._models_in_flight = True

        self._dispatch_control_request(
            verb="LIST_MODELS",
            payload="",
            show_error_popup=not silent,
            success_event_kind="models_list",
            read_timeout_s=8.0,
            inactivity_timeout_s=0.6,
            include_control_log=not silent,
            on_done=lambda: setattr(self, "_models_in_flight", False),
        )

    def _use_selected_pid(self):
        value = self.pid_combo.currentData()
        if value is None:
            return
        self.pid_input.setText(str(value))

    def _send_pid_signal(self, verb: str):
        pid = self.pid_input.text().strip()
        if not pid:
            self._show_error("Seleziona o inserisci un PID valido prima del segnale")
            return
        self._send_simple(verb, pid)

    def _selected_model_id(self) -> str:
        current = self.model_combo.currentData()
        if isinstance(current, str):
            return current
        return ""

    def _model_info(self):
        model_id = self._selected_model_id()
        if not model_id:
            self._show_error("Nessun modello selezionato")
            return
        self._send_simple("MODEL_INFO", model_id)

    def _select_model(self):
        model_id = self._selected_model_id()
        if not model_id:
            self._show_error("Nessun modello selezionato")
            return
        self._send_simple("SELECT_MODEL", model_id)

    def _load_selected_model(self):
        if self._load_in_flight:
            self.command_output.appendPlainText("[INFO] LOAD già in corso, attendi completamento.\n")
            return

        model_id = self._selected_model_id()
        if not model_id:
            self._show_error("Nessun modello selezionato")
            return

        self._load_in_flight = True
        self._set_status("Loading model...")
        self._set_controls_enabled(False)

        def task():
            try:
                self._update_client_config()
                select_resp = self._send_once_with_retry(
                    "SELECT_MODEL",
                    model_id,
                    read_timeout_s=8.0,
                    inactivity_timeout_s=0.6,
                )
                load_resp = self._send_once_with_retry(
                    "LOAD",
                    "",
                    read_timeout_s=self._load_read_timeout_s,
                    inactivity_timeout_s=self._load_inactivity_timeout_s,
                )
                self.ui_queue.put(
                    UiEvent(
                        kind="control",
                        message=self._format_control("SELECT_MODEL", select_resp),
                    )
                )
                self.ui_queue.put(
                    UiEvent(
                        kind="control",
                        message=self._format_control("LOAD", load_resp),
                    )
                )
                self.ui_queue.put(UiEvent(kind="refresh_after_load", message=""))
            except Exception as exc:
                self.ui_queue.put(UiEvent(kind="error", message=f"LOAD failed: {exc}"))
            finally:
                self._load_in_flight = False
                self.ui_queue.put(UiEvent(kind="load_done", message=""))

        threading.Thread(target=task, daemon=True).start()

    def _get_generation(self):
        self._send_with_event("GET_GEN", "", "gen_get")

    def _set_generation(self):
        seed = self.gen_seed.text().strip() or "42"
        max_tokens = self.gen_max_tokens.text().strip() or "512"
        payload = (
            f"temperature={self.gen_temp.value():.2f};"
            f"top_p={self.gen_top_p.value():.2f};"
            f"seed={seed};max_tokens={max_tokens}"
        )
        self._send_simple("SET_GEN", payload)

    def _exec_stream(self):
        prompt = self.exec_prompt.toPlainText().strip()
        if not prompt:
            self._show_error("Prompt EXEC vuoto")
            return

        if "no_model_loaded=true" in self.status_snapshot.toPlainText().lower():
            self._show_error("Nessun modello caricato: usa Control -> SELECT_MODEL + LOAD prima di EXEC")
            return

        self.exec_output.appendPlainText("\n--- EXEC start ---")

        def task():
            try:
                self._update_client_config()

                def on_frame(kind: str, code: str, body: bytes):
                    if kind == "DATA" and code.lower() == "raw":
                        text = body.decode("utf-8", errors="replace")
                        self.ui_queue.put(UiEvent(kind="exec_stream", message=text))
                    elif kind in {"+OK", "-ERR"}:
                        control = body.decode("utf-8", errors="replace")
                        self.ui_queue.put(
                            UiEvent(
                                kind="exec_control",
                                message=f"{kind} {code}: {control}",
                            )
                        )

                result = self._exec_stream_with_retry(prompt=prompt, on_frame=on_frame)
                self.ui_queue.put(
                    UiEvent(kind="exec_done", message=self._format_control("EXEC", result))
                )
            except Exception as exc:
                self.ui_queue.put(UiEvent(kind="error", message=f"EXEC failed: {exc}"))

        threading.Thread(target=task, daemon=True).start()

    def _drain_kernel_events(self):
        while True:
            try:
                event = self.kernel.events.get_nowait()
            except queue.Empty:
                break
            line = f"[{event.source}] {event.line}"
            self._kernel_event_lines.append(line)
            if len(self._kernel_event_lines) > self._max_log_lines:
                self._kernel_event_lines = self._kernel_event_lines[-self._max_log_lines :]

        self._render_kernel_events()

    def _tail_syscall_audit(self):
        audit_path = self.workspace_root / "workspace" / "syscall_audit.log"
        if not audit_path.exists():
            return

        try:
            with audit_path.open("r", encoding="utf-8", errors="replace") as handle:
                handle.seek(self._audit_offset)
                chunk = handle.read()
                self._audit_offset = handle.tell()
        except Exception:
            return

        if chunk:
            for line in chunk.splitlines():
                if line.strip():
                    self._syscall_event_lines.append(line)
            if len(self._syscall_event_lines) > self._max_log_lines:
                self._syscall_event_lines = self._syscall_event_lines[-self._max_log_lines :]
            self._render_syscall_events()

    def _flush_ui_queue(self):
        while True:
            try:
                event = self.ui_queue.get_nowait()
            except queue.Empty:
                break

            if event.kind == "error":
                self._is_connected = False
                self._show_error(event.message)
            elif event.kind == "background_error":
                self._is_connected = False
                if event.message != self._last_background_error:
                    self._last_background_error = event.message
                    self._set_status(f"Offline: {event.message}")
                    self.command_output.appendPlainText(f"[WARN] {event.message}\n")
            elif event.kind == "control":
                self.command_output.appendPlainText(event.message)
            elif event.kind == "status":
                self._is_connected = True
                self._last_background_error = ""
                self._set_status("Connected")
                self.status_snapshot.setPlainText(event.message)
                self._update_active_pids_from_status(event.message)
                self._update_model_status_from_status(event.message)
            elif event.kind == "models_list":
                self._populate_model_list(event.message)
            elif event.kind == "load_done":
                self._set_controls_enabled(True)
            elif event.kind == "refresh_after_load":
                self._refresh_status(force=True)
                self._refresh_models(silent=True, force=True)
            elif event.kind == "gen_get":
                self._apply_generation_payload(event.message)
            elif event.kind == "exec_stream":
                self.exec_output.insertPlainText(event.message)
                self.exec_output.ensureCursorVisible()
            elif event.kind in {"exec_control", "exec_done"}:
                self.exec_output.appendPlainText(f"\n{event.message}")
            else:
                self.command_output.appendPlainText(event.message)

    def _format_control(self, verb: str, response: ControlResponse) -> str:
        status = "OK" if response.ok else "ERR"
        return (
            f"[{verb}] status={status} code={response.code} duration={response.duration_s:.3f}s\n"
            f"{response.payload}\n"
        )

    def _populate_model_list(self, payload: str):
        previous = self._selected_model_id()
        self.model_combo.clear()
        for line in payload.splitlines():
            line = line.strip()
            match = re.search(r"id=([^\s]+)", line)
            if not match:
                continue
            model_id = match.group(1)
            label = self._format_model_label(line, model_id)
            self.model_combo.addItem(label, model_id)

        if self.model_combo.count() == 0:
            return

        if previous:
            for idx in range(self.model_combo.count()):
                if self.model_combo.itemData(idx) == previous:
                    self.model_combo.setCurrentIndex(idx)
                    return

        self.model_combo.setCurrentIndex(0)

    def _format_model_label(self, line: str, model_id: str) -> str:
        family_match = re.search(r"family=([^\s]+)", line)
        path_match = re.search(r"path=([^\s]+)", line)

        family = family_match.group(1) if family_match else "Unknown"
        source_name = model_id.split("/")[-1]
        if path_match:
            source_name = Path(path_match.group(1)).stem

        pretty = source_name.replace("_", " ").replace("-", " ").strip()
        pretty = re.sub(r"(?i)meta\s*", "", pretty)
        pretty = re.sub(r"(?i)qwen\s*2\.?5", "Qwen 2.5", pretty)
        pretty = re.sub(r"(?i)llama\s*3\.?1", "Llama 3.1", pretty)
        pretty = re.sub(r"(?i)\b(\d+)b\b", lambda m: f"{m.group(1)}B", pretty)
        pretty = re.sub(r"\s+", " ", pretty).strip()

        if not pretty:
            pretty = model_id

        return f"{pretty} ({family})"

    def _update_active_pids_from_status(self, payload: str):
        match = re.search(r"active_pids=\[([^\]]*)\]", payload)
        if not match:
            return

        content = match.group(1).strip()
        pids = [p.strip() for p in content.split(",") if p.strip()]
        self._active_pids = pids

        previous = self.pid_combo.currentData()
        self.pid_combo.clear()
        if not pids:
            self.pid_combo.addItem("active_pids: none", "")
            return

        for pid in pids:
            self.pid_combo.addItem(f"PID {pid}", pid)

        if previous:
            for idx in range(self.pid_combo.count()):
                if self.pid_combo.itemData(idx) == previous:
                    self.pid_combo.setCurrentIndex(idx)
                    return
        self.pid_combo.setCurrentIndex(0)

    def _extract_status_value(self, payload: str, key: str, default: str = "<none>") -> str:
        match = re.search(rf"\b{re.escape(key)}=([^\s]+)", payload)
        if not match:
            return default
        return match.group(1)

    def _update_model_status_from_status(self, payload: str):
        selected_model_id = self._extract_status_value(payload, "selected_model_id", "<none>")
        loaded_model_id = self._extract_status_value(payload, "loaded_model_id", "<none>")

        self.selected_model_value.setText(selected_model_id)
        self.loaded_model_value.setText(loaded_model_id)

        if loaded_model_id and loaded_model_id not in {"<none>", "<unknown>"}:
            for idx in range(self.model_combo.count()):
                if self.model_combo.itemData(idx) == loaded_model_id:
                    self.model_combo.setCurrentIndex(idx)
                    break

    def _apply_generation_payload(self, payload: str):
        kv = {}
        for token in payload.replace("\n", " ").split(" "):
            token = token.strip()
            if "=" not in token:
                continue
            key, value = token.split("=", 1)
            kv[key.strip()] = value.strip()

        try:
            if "temperature" in kv:
                self.gen_temp.setValue(float(kv["temperature"]))
            if "top_p" in kv:
                self.gen_top_p.setValue(float(kv["top_p"]))
            if "seed" in kv:
                self.gen_seed.setText(kv["seed"])
            if "max_tokens" in kv:
                self.gen_max_tokens.setText(kv["max_tokens"])
        except ValueError:
            pass

    def _show_error(self, message: str):
        self._set_status(f"Error: {message}")
        self.command_output.appendPlainText(f"[ERROR] {message}\n")
        QMessageBox.critical(self, "AgenticOS GUI", message)

    def _send_once_with_retry(
        self,
        verb: str,
        payload: str,
        read_timeout_s: float | None = None,
        inactivity_timeout_s: float | None = None,
    ) -> ControlResponse:
        last_exc: Exception | None = None
        timeout = read_timeout_s if read_timeout_s is not None else self._default_read_timeout_s
        inactivity_timeout = (
            inactivity_timeout_s
            if inactivity_timeout_s is not None
            else self._default_inactivity_timeout_s
        )
        for attempt in range(1, self._request_retries + 1):
            try:
                return self.client.send_once(
                    verb=verb,
                    payload=payload,
                    agent_id=self.agent_input.text().strip() or "1",
                    read_timeout_s=timeout,
                    inactivity_timeout_s=inactivity_timeout,
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
                    prompt=prompt,
                    agent_id=self.agent_input.text().strip() or "1",
                    on_frame=on_frame,
                )
            except Exception as exc:
                last_exc = exc
                if attempt < self._request_retries:
                    time.sleep(self._retry_delay_s)

        raise RuntimeError(
            f"EXEC stream failed after {self._request_retries} attempts: {last_exc}"
        )

    def _render_kernel_events(self):
        term = self.kernel_filter_input.text().strip().lower()
        allow_stdout = self.show_stdout_cb.isChecked()
        allow_stderr = self.show_stderr_cb.isChecked()
        show_noise = self.show_noise_cb.isChecked()

        filtered: list[str] = []
        for line in self._kernel_event_lines:
            if line.startswith("[stdout]") and not allow_stdout:
                continue
            if line.startswith("[stderr]") and not allow_stderr:
                continue
            if not show_noise and "New connection:" in line:
                continue
            if term and term not in line.lower():
                continue
            filtered.append(line)

        self.kernel_events.setPlainText("\n".join(filtered[-self._max_log_lines :]))
        self.kernel_events.verticalScrollBar().setValue(
            self.kernel_events.verticalScrollBar().maximum()
        )

    def _render_syscall_events(self):
        term = self.syscall_filter_input.text().strip().lower()
        filtered = [
            line
            for line in self._syscall_event_lines
            if not term or term in line.lower()
        ]
        self.syscall_log.setPlainText("\n".join(filtered[-self._max_log_lines :]))
        self.syscall_log.verticalScrollBar().setValue(
            self.syscall_log.verticalScrollBar().maximum()
        )

    def _export_snapshot(self):
        stamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
        out_dir = self.workspace_root / "reports"
        out_dir.mkdir(parents=True, exist_ok=True)
        out_file = out_dir / f"gui_snapshot_{stamp}.txt"

        summary = [
            f"timestamp={datetime.datetime.now().isoformat()}",
            f"host={self.host_input.text().strip() or '127.0.0.1'}",
            f"port={self.port_input.text().strip() or '6379'}",
            f"agent={self.agent_input.text().strip() or '1'}",
            f"kernel_running={self.kernel.is_running()}",
            "",
            "=== STATUS SNAPSHOT ===",
            self.status_snapshot.toPlainText(),
            "",
            "=== COMMAND LOG (tail) ===",
            self.command_output.toPlainText()[-10000:],
            "",
            "=== EXEC OUTPUT (tail) ===",
            self.exec_output.toPlainText()[-10000:],
            "",
            "=== KERNEL EVENTS (filtered view) ===",
            self.kernel_events.toPlainText()[-15000:],
            "",
            "=== SYSCALL LOG (filtered view) ===",
            self.syscall_log.toPlainText()[-15000:],
        ]

        out_file.write_text("\n".join(summary), encoding="utf-8")
        self.command_output.appendPlainText(f"[SNAPSHOT] exported: {out_file}\n")
        self._set_status(f"Snapshot exported: {out_file.name}")


def main():
    root = Path(__file__).resolve().parents[1]
    app = QApplication(sys.argv)
    window = MainWindow(workspace_root=root)
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
