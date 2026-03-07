from __future__ import annotations

import json

from PySide6.QtCore import Signal, Qt, QTimer
from PySide6.QtWidgets import (
    QComboBox,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

_ORCH_TEMPLATE = json.dumps(
    {
        "tasks": [
            {"id": "t1", "prompt": "hello", "workload": "chat", "deps": []},
            {"id": "t2", "prompt": "summarise t1", "workload": "chat", "deps": ["t1"]},
        ],
        "failure_policy": "fail_fast",
    },
    indent=2,
)


class OrchestrationSection(QWidget):
    """DAG task-graph editor, orchestration launcher, and live status viewer."""

    orchestrate_requested = Signal(str)      # json payload
    poll_orch_requested = Signal(str)        # orchestration_id
    refresh_requested = Signal()

    _TASK_COLS = ["Task", "Status", "PID", "Error"]

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(10)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Orchestration")
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

        # ── JSON editor ──────────────────────────────────────
        editor_frame = QFrame()
        editor_frame.setObjectName("card")
        editor_layout = QVBoxLayout(editor_frame)
        editor_layout.setContentsMargins(12, 10, 12, 10)
        editor_layout.setSpacing(6)

        editor_title = QLabel("Task Graph (JSON)")
        editor_title.setObjectName("card_title")
        editor_layout.addWidget(editor_title)

        self.json_editor = QPlainTextEdit()
        self.json_editor.setPlainText(_ORCH_TEMPLATE)
        self.json_editor.setMaximumHeight(200)
        editor_layout.addWidget(self.json_editor)

        submit_row = QHBoxLayout()
        policy_lbl = QLabel("Policy:")
        self.policy_combo = QComboBox()
        self.policy_combo.addItems(["fail_fast", "best_effort"])
        submit_row.addWidget(policy_lbl)
        submit_row.addWidget(self.policy_combo)
        submit_row.addStretch()

        submit_btn = QPushButton("Submit")
        submit_btn.setObjectName("primary_button")
        submit_btn.setFixedWidth(110)
        submit_btn.clicked.connect(self._on_submit)
        submit_row.addWidget(submit_btn)

        editor_layout.addLayout(submit_row)
        layout.addWidget(editor_frame)

        # ── Orchestration summary ────────────────────────────
        summary_frame = QFrame()
        summary_frame.setObjectName("card")
        summ_layout = QVBoxLayout(summary_frame)
        summ_layout.setContentsMargins(12, 10, 12, 10)
        summ_layout.setSpacing(6)

        summ_title = QLabel("Active Orchestration")
        summ_title.setObjectName("card_title")
        summ_layout.addWidget(summ_title)

        self.summ_label = QLabel("No active orchestration")
        self.summ_label.setObjectName("card_detail")
        self.summ_label.setWordWrap(True)
        summ_layout.addWidget(self.summ_label)

        poll_row = QHBoxLayout()
        self._orch_id_lbl = QLabel("")
        self._orch_id_lbl.setObjectName("mini_status_value")
        poll_row.addWidget(self._orch_id_lbl)
        poll_row.addStretch()

        poll_btn = QPushButton("Poll Status")
        poll_btn.setFixedWidth(110)
        poll_btn.clicked.connect(self._on_poll)
        poll_row.addWidget(poll_btn)
        summ_layout.addLayout(poll_row)

        layout.addWidget(summary_frame)

        # ── Task table ───────────────────────────────────────
        self.task_table = QTableWidget(0, len(self._TASK_COLS))
        self.task_table.setHorizontalHeaderLabels(self._TASK_COLS)
        self.task_table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.task_table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.task_table.verticalHeader().setVisible(False)
        hh = self.task_table.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        hh.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        hh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        hh.setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)
        self.task_table.setMaximumHeight(200)
        layout.addWidget(self.task_table)

        layout.addStretch()

        self._current_orch_id: str = ""

        # Auto-poll timer
        self._poll_timer = QTimer(self)
        self._poll_timer.timeout.connect(self._on_poll)
        self._orch_finished = True

    # ── Public API ───────────────────────────────────────────

    def update_orch_status(self, data: dict):
        """Update UI from orchestration STATUS dict (JSON-decoded)."""
        orch_id = str(data.get("orchestration_id", ""))
        if orch_id:
            self._current_orch_id = orch_id
            self._orch_id_lbl.setText(f"ID: {orch_id}")

        total = data.get("total", 0)
        completed = data.get("completed", 0)
        running = data.get("running", 0)
        pending = data.get("pending", 0)
        failed = data.get("failed", 0)
        skipped = data.get("skipped", 0)
        finished = data.get("finished", False)
        elapsed = data.get("elapsed_secs", "?")
        policy = data.get("policy", "?")

        if finished:
            self._orch_finished = True
            self._poll_timer.stop()

        self.summ_label.setText(
            f"Total {total}  •  Completed {completed}  •  Running {running}  •  "
            f"Pending {pending}  •  Failed {failed}  •  Skipped {skipped}\n"
            f"Finished: {finished}  •  Elapsed: {elapsed}s  •  Policy: {policy}"
        )

        # Parse per-task entries from JSON array
        tasks = data.get("tasks", [])
        self.task_table.setRowCount(len(tasks))
        for row, entry in enumerate(tasks):
            task_id = str(entry.get("task", "?"))
            status = str(entry.get("status", "?"))
            pid = str(entry.get("pid", "—")) if entry.get("pid") is not None else "—"
            error = str(entry.get("error", "")) if entry.get("error") is not None else ""
            for col, val in enumerate([task_id, status, pid, error]):
                item = QTableWidgetItem(val)
                if col == 1:
                    item.setForeground(self._status_color(status))
                self.task_table.setItem(row, col, item)

    def show_submit_result(self, data: dict):
        """Show result of ORCHESTRATE command (JSON-decoded dict)."""
        orch_id = str(data.get("orchestration_id", ""))
        if orch_id:
            self._current_orch_id = orch_id
            self._orch_id_lbl.setText(f"ID: {orch_id}")
        total = data.get("total_tasks", "?")
        spawned = data.get("spawned", "?")
        self.summ_label.setText(
            f"Submitted — ID: {orch_id}  •  Tasks: {total}  •  Spawned: {spawned}"
        )
        # Start auto-polling
        self._orch_finished = False
        if not self._poll_timer.isActive():
            self._poll_timer.start(3000)

    def show_error(self, text: str):
        self.error_banner.setText(f"⚠ {text}")
        self.error_banner.setVisible(True)
        QTimer.singleShot(6000, lambda: self.error_banner.setVisible(False))

    # ── Internals ────────────────────────────────────────────

    def _on_submit(self):
        raw = self.json_editor.toPlainText().strip()
        if not raw:
            return
        try:
            obj = json.loads(raw)
        except json.JSONDecodeError:
            self.summ_label.setText("Invalid JSON — please fix the task graph")
            return
        # Inject chosen failure_policy
        obj["failure_policy"] = self.policy_combo.currentText()
        self.orchestrate_requested.emit(json.dumps(obj))

    def _on_poll(self):
        if self._current_orch_id:
            self.poll_orch_requested.emit(self._current_orch_id)

    @staticmethod
    def _status_color(status: str):
        from PySide6.QtGui import QColor
        mapping = {
            "completed": QColor("#73daca"),
            "running": QColor("#7aa2f7"),
            "pending": QColor("#a9b1d6"),
            "failed": QColor("#f7768e"),
            "skipped": QColor("#565f89"),
        }
        return mapping.get(status, QColor("#c0caf5"))


