from __future__ import annotations

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

    def update_from_status(self, data: dict):
        """Parse global STATUS dict (JSON-decoded) and populate process table."""
        procs = data.get("processes", {})
        active_pids = [str(p) for p in procs.get("active_pids", [])]
        waiting_pids = [str(p) for p in procs.get("waiting_pids", [])]
        all_pids = active_pids + waiting_pids

        # Scheduler summary
        sched = data.get("scheduler", {})
        tracked = sched.get("tracked", 0)
        crit = sched.get("priority_critical", 0)
        high = sched.get("priority_high", 0)
        norm = sched.get("priority_normal", 0)
        low = sched.get("priority_low", 0)
        self.scheduler_label.setText(
            f"Scheduler: {tracked} tracked  │  "
            f"Critical: {crit}  High: {high}  Normal: {norm}  Low: {low}"
        )

        # Snapshot current cell values keyed by PID so we can preserve them
        _prev: dict[str, list[str]] = {}
        for row in range(self.table.rowCount()):
            item0 = self.table.item(row, 0)
            if item0:
                _prev[item0.text()] = [
                    (self.table.item(row, c).text() if self.table.item(row, c) else "—")
                    for c in range(self.table.columnCount())
                ]

        # Update table
        prev_pid = self._selected_pid()
        self.table.setRowCount(len(all_pids))

        # Build lookup from active_processes (per-PID detail embedded in global STATUS)
        _detail: dict[str, dict] = {}
        for proc in procs.get("active_processes", []):
            _detail[str(proc.get("pid", ""))] = proc

        for row, pid in enumerate(all_pids):
            state = "active" if pid in active_pids else "waiting"
            detail = _detail.get(pid)
            if detail:
                workload = str(detail.get("workload", "—"))
                priority = str(detail.get("priority", "—"))
                tokens_gen = str(detail.get("tokens_generated", "—"))
                quota_tokens = str(detail.get("quota_tokens", "—"))
                syscalls_used = str(detail.get("syscalls_used", "—"))
                quota_syscalls = str(detail.get("quota_syscalls", "—"))
                elapsed = detail.get("elapsed_secs", "—")
                tok_cell = f"{tokens_gen}/{quota_tokens}"
                sys_cell = f"{syscalls_used}/{quota_syscalls}"
                try:
                    elapsed_cell = f"{float(elapsed):.1f}s"
                except (ValueError, TypeError):
                    elapsed_cell = str(elapsed)
            else:
                old = _prev.get(pid)
                workload = old[1] if old else "—"
                priority = old[2] if old else "—"
                tok_cell = old[4] if old else "—"
                sys_cell = old[5] if old else "—"
                elapsed_cell = old[6] if old else "—"
            self.table.setItem(row, 0, QTableWidgetItem(pid))
            self.table.setItem(row, 1, QTableWidgetItem(workload))
            self.table.setItem(row, 2, QTableWidgetItem(priority))
            self.table.setItem(row, 3, QTableWidgetItem(state))
            self.table.setItem(row, 4, QTableWidgetItem(tok_cell))
            self.table.setItem(row, 5, QTableWidgetItem(sys_cell))
            self.table.setItem(row, 6, QTableWidgetItem(elapsed_cell))

        # Restore selection
        if prev_pid:
            for row in range(self.table.rowCount()):
                item = self.table.item(row, 0)
                if item and item.text() == prev_pid:
                    self.table.setCurrentCell(row, 0)
                    break

        # Update detail panel for currently selected PID (if it has inline data)
        sel = self._selected_pid()
        if sel and sel in _detail:
            self.update_pid_detail(sel, _detail[sel])

    def update_pid_detail(self, pid: str, data: dict):
        """Update row + detail from per-PID STATUS dict (JSON-decoded)."""
        if not pid:
            pid = str(data.get("pid", ""))
        if not pid:
            return

        row = self._find_row(pid)

        priority = str(data.get("priority", "—"))
        workload = str(data.get("workload", "—"))
        tokens_gen = str(data.get("tokens_generated", "—"))
        syscalls_used = str(data.get("syscalls_used", "—"))
        elapsed = str(data.get("elapsed_secs", "—"))
        quota_tokens = str(data.get("quota_tokens", data.get("max_tokens", "—")))
        quota_syscalls = str(data.get("quota_syscalls", data.get("max_syscalls", "—")))

        if row >= 0:
            self.table.setItem(row, 1, QTableWidgetItem(workload))
            self.table.setItem(row, 2, QTableWidgetItem(priority))
            self.table.setItem(row, 4, QTableWidgetItem(f"{tokens_gen}/{quota_tokens}"))
            self.table.setItem(row, 5, QTableWidgetItem(f"{syscalls_used}/{quota_syscalls}"))
            try:
                self.table.setItem(row, 6, QTableWidgetItem(f"{float(elapsed):.1f}s"))
            except (ValueError, TypeError):
                self.table.setItem(row, 6, QTableWidgetItem(elapsed))

        # Only update detail panel + controls for the currently selected PID
        if pid == self._selected_pid():
            self.detail_label.setText(
                f"PID {pid}  │  priority={priority}  workload={workload}\n"
                f"tokens: {tokens_gen}/{quota_tokens}  │  syscalls: {syscalls_used}/{quota_syscalls}  │  elapsed: {elapsed}s"
            )
            idx = self.priority_combo.findText(priority.lower())
            if idx >= 0:
                self.priority_combo.setCurrentIndex(idx)
            if quota_tokens not in {"—", ""}:
                self.quota_tokens.setText(str(quota_tokens))
            if quota_syscalls not in {"—", ""}:
                self.quota_syscalls.setText(str(quota_syscalls))

    def show_quota(self, data: dict):
        """Parse GET_QUOTA JSON response dict."""
        pid = str(data.get("pid", "?"))
        self.update_pid_detail(pid, data)

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


