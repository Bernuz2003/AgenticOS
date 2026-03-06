from __future__ import annotations

import datetime
import re
from pathlib import Path

from PySide6.QtCore import Signal, Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QPushButton,
    QSplitter,
    QVBoxLayout,
    QWidget,
)


_MAX_LOG_LINES = 4000
_ANSI_RE = re.compile(r'\x1b\[[0-9;]*m')


class LogsSection(QWidget):
    """Kernel events + syscall audit log viewer with filtering and export."""

    export_requested = Signal()

    def __init__(self, workspace_root: Path, parent: QWidget | None = None):
        super().__init__(parent)
        self._workspace_root = workspace_root
        self._kernel_lines: list[str] = []
        self._syscall_lines: list[str] = []
        self._audit_offset = 0

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(8)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Logs")
        title.setObjectName("section_title")
        header.addWidget(title)
        header.addStretch()

        self.export_btn = QPushButton("Export Snapshot")
        self.export_btn.clicked.connect(self._export_snapshot)
        header.addWidget(self.export_btn)
        layout.addLayout(header)

        # ── Filter bar ───────────────────────────────────────
        filter_row = QHBoxLayout()

        filter_row.addWidget(QLabel("Kernel:"))
        self.kernel_filter = QLineEdit()
        self.kernel_filter.setPlaceholderText("Filter kernel events...")
        self.kernel_filter.textChanged.connect(self._render_kernel)
        filter_row.addWidget(self.kernel_filter)

        self.show_stdout = QCheckBox("stdout")
        self.show_stdout.setChecked(True)
        self.show_stdout.toggled.connect(self._render_kernel)
        filter_row.addWidget(self.show_stdout)

        self.show_stderr = QCheckBox("stderr")
        self.show_stderr.setChecked(True)
        self.show_stderr.toggled.connect(self._render_kernel)
        filter_row.addWidget(self.show_stderr)

        self.show_noise = QCheckBox("noise")
        self.show_noise.setChecked(False)
        self.show_noise.toggled.connect(self._render_kernel)
        filter_row.addWidget(self.show_noise)

        filter_row.addSpacing(16)
        filter_row.addWidget(QLabel("Syscall:"))
        self.syscall_filter = QLineEdit()
        self.syscall_filter.setPlaceholderText("Filter syscall audit...")
        self.syscall_filter.textChanged.connect(self._render_syscall)
        filter_row.addWidget(self.syscall_filter)

        layout.addLayout(filter_row)

        # ── Log panes ────────────────────────────────────────
        splitter = QSplitter(Qt.Orientation.Horizontal)

        self.kernel_log = QPlainTextEdit()
        self.kernel_log.setReadOnly(True)
        self.kernel_log.setPlaceholderText("Kernel stdout/stderr events...")

        self.syscall_log = QPlainTextEdit()
        self.syscall_log.setReadOnly(True)
        self.syscall_log.setPlaceholderText("Tail of workspace/syscall_audit.log...")

        self.protocol_log = QPlainTextEdit()
        self.protocol_log.setReadOnly(True)
        self.protocol_log.setPlaceholderText("Protocol trace (req → resp)...")

        splitter.addWidget(self.kernel_log)
        splitter.addWidget(self.syscall_log)
        splitter.addWidget(self.protocol_log)
        splitter.setSizes([500, 350, 350])

        layout.addWidget(splitter, stretch=1)
        self._protocol_lines: list[str] = []

    # ── Public API (called from MainWindow timers) ───────────

    def append_kernel_event(self, source: str, text: str):
        """Append a single kernel event line from kernel_manager."""
        clean = _ANSI_RE.sub('', text)
        line = f"[{source}] {clean}"
        self._kernel_lines.append(line)
        if len(self._kernel_lines) > _MAX_LOG_LINES:
            self._kernel_lines = self._kernel_lines[-_MAX_LOG_LINES:]
        self._render_kernel()

    def poll_syscall_audit(self):
        """Incrementally read new lines from syscall_audit.log."""
        audit_path = self._workspace_root / "workspace" / "syscall_audit.log"
        if not audit_path.exists():
            return
        try:
            with audit_path.open("r", encoding="utf-8", errors="replace") as fh:
                fh.seek(self._audit_offset)
                chunk = fh.read()
                self._audit_offset = fh.tell()
        except Exception:
            return

        if not chunk:
            return
        for line in chunk.splitlines():
            stripped = line.strip()
            if stripped:
                self._syscall_lines.append(stripped)
        if len(self._syscall_lines) > _MAX_LOG_LINES:
            self._syscall_lines = self._syscall_lines[-_MAX_LOG_LINES:]
        self._render_syscall()

    def get_kernel_text(self) -> str:
        return self.kernel_log.toPlainText()

    def get_syscall_text(self) -> str:
        return self.syscall_log.toPlainText()

    def append_protocol_trace(self, verb: str, payload: str, response: str):
        """Log a protocol request→response pair."""
        ts = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        req_short = payload[:120].replace("\n", "\\n") if payload else ""
        resp_short = response[:200].replace("\n", "\\n") if response else ""
        line = f"[{ts}] {verb} {req_short} → {resp_short}"
        self._protocol_lines.append(line)
        if len(self._protocol_lines) > _MAX_LOG_LINES:
            self._protocol_lines = self._protocol_lines[-_MAX_LOG_LINES:]
        self.protocol_log.setPlainText("\n".join(self._protocol_lines))
        sb = self.protocol_log.verticalScrollBar()
        sb.setValue(sb.maximum())

    # ── Internals ────────────────────────────────────────────

    def _render_kernel(self):
        term = self.kernel_filter.text().strip().lower()
        allow_stdout = self.show_stdout.isChecked()
        allow_stderr = self.show_stderr.isChecked()
        noise = self.show_noise.isChecked()

        filtered: list[str] = []
        for line in self._kernel_lines:
            if line.startswith("[stdout]") and not allow_stdout:
                continue
            if line.startswith("[stderr]") and not allow_stderr:
                continue
            if not noise and "New connection:" in line:
                continue
            if term and term not in line.lower():
                continue
            filtered.append(line)

        self.kernel_log.setPlainText("\n".join(filtered[-_MAX_LOG_LINES:]))
        sb = self.kernel_log.verticalScrollBar()
        sb.setValue(sb.maximum())

    def _render_syscall(self):
        term = self.syscall_filter.text().strip().lower()
        filtered = [
            line for line in self._syscall_lines
            if not term or term in line.lower()
        ]
        self.syscall_log.setPlainText("\n".join(filtered[-_MAX_LOG_LINES:]))
        sb = self.syscall_log.verticalScrollBar()
        sb.setValue(sb.maximum())

    def _export_snapshot(self):
        stamp = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
        out_dir = self._workspace_root / "reports"
        out_dir.mkdir(parents=True, exist_ok=True)
        out_file = out_dir / f"gui_snapshot_{stamp}.txt"

        summary = [
            f"timestamp={datetime.datetime.now().isoformat()}",
            "",
            "=== KERNEL EVENTS (filtered view) ===",
            self.kernel_log.toPlainText()[-15000:],
            "",
            "=== SYSCALL LOG (filtered view) ===",
            self.syscall_log.toPlainText()[-15000:],
            "",
            "=== PROTOCOL TRACE ===",
            self.protocol_log.toPlainText()[-15000:],
        ]
        out_file.write_text("\n".join(summary), encoding="utf-8")
        self.export_requested.emit()
