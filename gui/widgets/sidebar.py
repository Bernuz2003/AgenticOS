from __future__ import annotations

from PySide6.QtCore import Signal, Qt
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

SECTIONS = [
    ("Chat", "💬"),
    ("Models", "🧠"),
    ("Processes", "⚙"),
    ("Memory", "🗄"),
    ("Orchestration", "🔀"),
    ("Logs", "📋"),
]


class SidebarWidget(QWidget):
    """Sidebar with navigation buttons and live kernel mini-status."""

    section_changed = Signal(int)
    start_kernel = Signal()
    stop_kernel = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self.setObjectName("sidebar")
        self.setFixedWidth(210)
        self._active_index = 0
        self._nav_buttons: list[QPushButton] = []

        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 16, 10, 12)
        layout.setSpacing(2)

        # ── Title ────────────────────────────────────────────
        title = QLabel("AgenticOS")
        title.setObjectName("sidebar_title")
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(title)

        version = QLabel("Control Center")
        version.setObjectName("section_subtitle")
        version.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(version)

        layout.addSpacing(16)

        # ── Nav buttons ──────────────────────────────────────
        for idx, (name, icon) in enumerate(SECTIONS):
            btn = QPushButton(f"  {icon}  {name}")
            btn.setObjectName("nav_button")
            btn.setCursor(Qt.CursorShape.PointingHandCursor)
            btn.clicked.connect(lambda checked=False, i=idx: self._on_nav(i))
            layout.addWidget(btn)
            self._nav_buttons.append(btn)

        layout.addStretch()

        # ── Mini-status ──────────────────────────────────────
        sep = QFrame()
        sep.setFrameShape(QFrame.Shape.HLine)
        sep.setStyleSheet("color: #3b4261;")
        layout.addWidget(sep)

        self._status_indicator = QLabel("● Offline")
        self._status_indicator.setObjectName("status_offline")
        layout.addWidget(self._status_indicator)

        status_grid = QVBoxLayout()
        status_grid.setSpacing(2)

        self._model_label = self._make_status_row("Model", "—")
        self._procs_label = self._make_status_row("Procs", "0")
        self._uptime_label = self._make_status_row("Uptime", "—")

        for row in [self._model_label, self._procs_label, self._uptime_label]:
            status_grid.addLayout(row)

        layout.addLayout(status_grid)
        layout.addSpacing(8)

        # ── Kernel buttons ───────────────────────────────────
        btn_row = QHBoxLayout()
        start_btn = QPushButton("Start")
        start_btn.setObjectName("start_button")
        start_btn.setFixedHeight(28)
        start_btn.clicked.connect(self.start_kernel.emit)

        stop_btn = QPushButton("Stop")
        stop_btn.setObjectName("stop_button")
        stop_btn.setFixedHeight(28)
        stop_btn.clicked.connect(self.stop_kernel.emit)

        btn_row.addWidget(start_btn)
        btn_row.addWidget(stop_btn)
        layout.addLayout(btn_row)

        # Highlight first section
        self._highlight(0)

    # ── Public API ───────────────────────────────────────────

    def update_status(
        self,
        online: bool,
        model_name: str = "—",
        proc_count: int = 0,
        uptime: str = "—",
    ):
        if online:
            self._status_indicator.setText("● Online")
            self._status_indicator.setObjectName("status_online")
        else:
            self._status_indicator.setText("● Offline")
            self._status_indicator.setObjectName("status_offline")
        # Force style refresh after objectName change
        self._status_indicator.style().unpolish(self._status_indicator)
        self._status_indicator.style().polish(self._status_indicator)

        self._set_status_value(self._model_label, model_name)
        self._set_status_value(self._procs_label, str(proc_count))
        self._set_status_value(self._uptime_label, uptime)

    # ── Internals ────────────────────────────────────────────

    def _on_nav(self, index: int):
        self._active_index = index
        self._highlight(index)
        self.section_changed.emit(index)

    def _highlight(self, index: int):
        for i, btn in enumerate(self._nav_buttons):
            btn.setProperty("active", i == index)
            btn.style().unpolish(btn)
            btn.style().polish(btn)

    def _make_status_row(self, label_text: str, value_text: str) -> QHBoxLayout:
        row = QHBoxLayout()
        lbl = QLabel(label_text)
        lbl.setObjectName("mini_status_label")
        val = QLabel(value_text)
        val.setObjectName("mini_status_value")
        row.addWidget(lbl)
        row.addStretch()
        row.addWidget(val)
        return row

    @staticmethod
    def _set_status_value(row_layout: QHBoxLayout, text: str):
        val_widget = row_layout.itemAt(row_layout.count() - 1).widget()
        if isinstance(val_widget, QLabel):
            val_widget.setText(text)
