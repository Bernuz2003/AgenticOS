from __future__ import annotations

import re

from PySide6.QtCore import Signal, Qt, QTimer
from PySide6.QtWidgets import (
    QComboBox,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

_PRIORITY_LEVELS = ["low", "normal", "high", "critical"]

_PROC_COLUMNS = ["PID", "Workload", "Priority", "State", "Tokens", "Syscalls", "Elapsed"]


class ProcessesSection(QWidget):
    """Active processes table with priority / quota controls and TERM / KILL."""

    set_priority_requested = Signal(str, str)   # (pid, priority)
    set_quota_requested = Signal(str, str)      # (pid, payload)
    get_quota_requested = Signal(str)           # pid
    term_requested = Signal(str)                # pid
    kill_requested = Signal(str)                # pid
    status_pid_requested = Signal(str)          # pid (for detail refresh)
    refresh_requested = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(10)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Processes")
        title.setObjectName("section_title")
        header.addWidget(title)
        header.addStretch()
        refresh_btn = QPushButton("Refresh")
        refresh_btn.clicked.connect(self.refresh_requested.emit)
        header.addWidget(refresh_btn)
        layout.addLayout(header)

        # ── Error banner ─────────────────────────────────────
        self.error_banner = QLabel()
        self.error_banner.setObjectName("error_banner")
        self.error_banner.setWordWrap(True)
        self.error_banner.setVisible(False)
        layout.addWidget(self.error_banner)

        # ── Process table ────────────────────────────────────
        self.table = QTableWidget(0, len(_PROC_COLUMNS))
        self.table.setHorizontalHeaderLabels(_PROC_COLUMNS)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.setSelectionMode(QTableWidget.SelectionMode.SingleSelection)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.horizontalHeader().setStretchLastSection(True)
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        self.table.verticalHeader().setVisible(False)
        self.table.currentCellChanged.connect(self._on_row_changed)
        layout.addWidget(self.table, stretch=1)

        # ── Detail / control panel ───────────────────────────
        detail_frame = QFrame()
        detail_frame.setObjectName("card")
        detail_layout = QVBoxLayout(detail_frame)
        detail_layout.setContentsMargins(12, 10, 12, 10)
        detail_layout.setSpacing(8)

        detail_title = QLabel("Process Detail")
        detail_title.setObjectName("card_title")
        detail_layout.addWidget(detail_title)

        self.detail_label = QLabel("Select a process above.")
        self.detail_label.setObjectName("card_detail")
        self.detail_label.setWordWrap(True)
        detail_layout.addWidget(self.detail_label)

        # ── Priority row ─────────────────────────────────────
        prio_row = QHBoxLayout()
        prio_row.addWidget(QLabel("Priority:"))
        self.priority_combo = QComboBox()
        self.priority_combo.addItems(_PRIORITY_LEVELS)
        self.priority_combo.setFixedWidth(120)
        prio_row.addWidget(self.priority_combo)

        set_prio_btn = QPushButton("SET_PRIORITY")
        set_prio_btn.setObjectName("primary_button")
        set_prio_btn.setFixedWidth(130)
        set_prio_btn.clicked.connect(self._on_set_priority)
        prio_row.addWidget(set_prio_btn)
        prio_row.addStretch()
        detail_layout.addLayout(prio_row)

        # ── Quota row ────────────────────────────────────────
        quota_row = QHBoxLayout()
        quota_row.addWidget(QLabel("max_tokens:"))
        self.quota_tokens = QLineEdit()
        self.quota_tokens.setPlaceholderText("e.g. 4096")
        self.quota_tokens.setFixedWidth(80)
        quota_row.addWidget(self.quota_tokens)

        quota_row.addWidget(QLabel("max_syscalls:"))
        self.quota_syscalls = QLineEdit()
        self.quota_syscalls.setPlaceholderText("e.g. 16")
        self.quota_syscalls.setFixedWidth(80)
        quota_row.addWidget(self.quota_syscalls)

        set_quota_btn = QPushButton("SET_QUOTA")
        set_quota_btn.setFixedWidth(110)
        set_quota_btn.clicked.connect(self._on_set_quota)
        quota_row.addWidget(set_quota_btn)

        get_quota_btn = QPushButton("GET_QUOTA")
        get_quota_btn.setFixedWidth(110)
        get_quota_btn.clicked.connect(self._on_get_quota)
        quota_row.addWidget(get_quota_btn)

        quota_row.addStretch()
        detail_layout.addLayout(quota_row)

        # ── Action row ───────────────────────────────────────
        action_row = QHBoxLayout()
        term_btn = QPushButton("TERM")
        term_btn.setFixedWidth(80)
        term_btn.clicked.connect(lambda: self.term_requested.emit(self._selected_pid()))
        action_row.addWidget(term_btn)

        kill_btn = QPushButton("KILL")
        kill_btn.setObjectName("danger_button")
        kill_btn.setFixedWidth(80)
        kill_btn.clicked.connect(lambda: self.kill_requested.emit(self._selected_pid()))
        action_row.addWidget(kill_btn)

        detail_btn = QPushButton("STATUS <PID>")
        detail_btn.setFixedWidth(120)
        detail_btn.clicked.connect(lambda: self.status_pid_requested.emit(self._selected_pid()))
        action_row.addWidget(detail_btn)

        action_row.addStretch()
        detail_layout.addLayout(action_row)

        layout.addWidget(detail_frame)

        # ── Scheduler summary ────────────────────────────────
        self.scheduler_label = QLabel("Scheduler: —")
        self.scheduler_label.setObjectName("card_detail")
        layout.addWidget(self.scheduler_label)

    # ── Public API ───────────────────────────────────────────

    def update_from_status(self, payload: str):
        """Parse global STATUS payload and populate process table."""
        # Extract PIDs
        pid_match = re.search(r"active_pids=\[([^\]]*)\]", payload)
        active_pids = []
        if pid_match:
            content = pid_match.group(1).strip()
            active_pids = [p.strip() for p in content.split(",") if p.strip()]

        waiting_match = re.search(r"waiting_pids=\[([^\]]*)\]", payload)
        waiting_pids = []
        if waiting_match:
            content = waiting_match.group(1).strip()
            waiting_pids = [p.strip() for p in content.split(",") if p.strip()]

        all_pids = active_pids + waiting_pids

        # Scheduler summary
        tracked = self._ex(payload, "scheduler_tracked", "0")
        crit = self._ex(payload, "priority_critical", "0")
        high = self._ex(payload, "priority_high", "0")
        norm = self._ex(payload, "priority_normal", "0")
        low = self._ex(payload, "priority_low", "0")
        self.scheduler_label.setText(
            f"Scheduler: {tracked} tracked  │  "
            f"Critical: {crit}  High: {high}  Normal: {norm}  Low: {low}"
        )

        # Update table
        prev_pid = self._selected_pid()
        self.table.setRowCount(len(all_pids))
        for row, pid in enumerate(all_pids):
            state = "active" if pid in active_pids else "waiting"
            self.table.setItem(row, 0, QTableWidgetItem(pid))
            self.table.setItem(row, 1, QTableWidgetItem("—"))      # workload — need per-PID STATUS
            self.table.setItem(row, 2, QTableWidgetItem("—"))      # priority
            self.table.setItem(row, 3, QTableWidgetItem(state))
            self.table.setItem(row, 4, QTableWidgetItem("—"))      # tokens
            self.table.setItem(row, 5, QTableWidgetItem("—"))      # syscalls
            self.table.setItem(row, 6, QTableWidgetItem("—"))      # elapsed

        # Restore selection
        if prev_pid:
            for row in range(self.table.rowCount()):
                item = self.table.item(row, 0)
                if item and item.text() == prev_pid:
                    self.table.setCurrentCell(row, 0)
                    break

    def update_pid_detail(self, pid: str, payload: str):
        """Parse per-PID STATUS or GET_QUOTA response and update row + detail."""
        # Try to find row for this PID
        row = self._find_row(pid)

        priority = self._ex(payload, "priority", "—")
        workload = self._ex(payload, "workload", "—")
        tokens_gen = self._ex(payload, "tokens_generated", "—")
        syscalls_used = self._ex(payload, "syscalls_used", "—")
        elapsed = self._ex(payload, "elapsed_secs", "—")
        max_tokens = self._ex(payload, "max_tokens", "—")
        max_syscalls = self._ex(payload, "max_syscalls", "—")
        quota_tokens = self._ex(payload, "quota_tokens", max_tokens)
        quota_syscalls = self._ex(payload, "quota_syscalls", max_syscalls)

        if row >= 0:
            self.table.setItem(row, 1, QTableWidgetItem(workload))
            self.table.setItem(row, 2, QTableWidgetItem(priority))
            self.table.setItem(row, 4, QTableWidgetItem(f"{tokens_gen}/{quota_tokens}"))
            self.table.setItem(row, 5, QTableWidgetItem(f"{syscalls_used}/{quota_syscalls}"))
            try:
                self.table.setItem(row, 6, QTableWidgetItem(f"{float(elapsed):.1f}s"))
            except (ValueError, TypeError):
                self.table.setItem(row, 6, QTableWidgetItem(elapsed))

        self.detail_label.setText(
            f"PID {pid}  │  priority={priority}  workload={workload}\n"
            f"tokens: {tokens_gen}/{quota_tokens}  │  syscalls: {syscalls_used}/{quota_syscalls}  │  elapsed: {elapsed}s"
        )

        # Pre-fill controls
        idx = self.priority_combo.findText(priority.lower())
        if idx >= 0:
            self.priority_combo.setCurrentIndex(idx)
        if quota_tokens not in {"—", ""}:
            self.quota_tokens.setText(str(quota_tokens))
        if quota_syscalls not in {"—", ""}:
            self.quota_syscalls.setText(str(quota_syscalls))

    def show_quota(self, payload: str):
        """Parse GET_QUOTA response."""
        pid = self._ex(payload, "pid", "?")
        self.update_pid_detail(pid, payload)

    def show_error(self, text: str):
        self.error_banner.setText(f"⚠ {text}")
        self.error_banner.setVisible(True)
        QTimer.singleShot(6000, lambda: self.error_banner.setVisible(False))

    # ── Internals ────────────────────────────────────────────

    def _selected_pid(self) -> str:
        row = self.table.currentRow()
        if row < 0:
            return ""
        item = self.table.item(row, 0)
        return item.text() if item else ""

    def _find_row(self, pid: str) -> int:
        for row in range(self.table.rowCount()):
            item = self.table.item(row, 0)
            if item and item.text() == pid:
                return row
        return -1

    def _on_row_changed(self, row: int, _col: int, _prev_row: int, _prev_col: int):
        pid = self._selected_pid()
        if pid:
            self.status_pid_requested.emit(pid)

    def _on_set_priority(self):
        pid = self._selected_pid()
        if not pid:
            return
        self.set_priority_requested.emit(pid, self.priority_combo.currentText())

    def _on_set_quota(self):
        pid = self._selected_pid()
        if not pid:
            return
        parts = []
        tok = self.quota_tokens.text().strip()
        sys_ = self.quota_syscalls.text().strip()
        if tok:
            parts.append(f"max_tokens={tok}")
        if sys_:
            parts.append(f"max_syscalls={sys_}")
        if parts:
            self.set_quota_requested.emit(pid, ','.join(parts))

    def _on_get_quota(self):
        pid = self._selected_pid()
        if pid:
            self.get_quota_requested.emit(pid)

    @staticmethod
    def _ex(payload: str, key: str, default: str = "") -> str:
        match = re.search(rf"\b{re.escape(key)}=([^\s]+)", payload)
        return match.group(1) if match else default
