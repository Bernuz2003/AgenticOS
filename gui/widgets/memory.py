from __future__ import annotations

from PySide6.QtCore import Signal, Qt, QTimer
from PySide6.QtWidgets import (
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QProgressBar,
    QPushButton,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)


class MemorySection(QWidget):
    """NeuralMemory usage, swap status, CHECKPOINT/RESTORE, and MEMW form."""

    checkpoint_requested = Signal(str)          # optional path
    restore_requested = Signal(str)             # optional path
    memw_requested = Signal(str, str)       # (pid, raw_text)
    refresh_requested = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(10)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Memory")
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

        # ── Usage bar ────────────────────────────────────────
        usage_frame = QFrame()
        usage_frame.setObjectName("card")
        usage_layout = QVBoxLayout(usage_frame)
        usage_layout.setContentsMargins(12, 10, 12, 10)
        usage_layout.setSpacing(6)

        usage_title = QLabel("NeuralMemory Usage")
        usage_title.setObjectName("card_title")
        usage_layout.addWidget(usage_title)

        self.usage_bar = QProgressBar()
        self.usage_bar.setRange(0, 100)
        self.usage_bar.setValue(0)
        self.usage_bar.setTextVisible(True)
        self.usage_bar.setFormat("0 / 0 blocks (%p%)")
        usage_layout.addWidget(self.usage_bar)

        # Stats grid
        self._stats: dict[str, QLabel] = {}
        stats_grid = QGridLayout()
        stats_grid.setSpacing(4)
        stat_keys = [
            ("active", "Active"),
            ("total_blocks", "Total blocks"),
            ("free_blocks", "Free blocks"),
            ("allocated_tensors", "Tensors"),
            ("tracked_pids", "Tracked PIDs"),
            ("alloc_bytes", "Alloc bytes"),
            ("evictions", "Evictions"),
            ("oom_events", "OOM events"),
        ]
        for i, (key, label) in enumerate(stat_keys):
            row, col = divmod(i, 4)
            lbl = QLabel(f"{label}:")
            lbl.setObjectName("mini_status_label")
            val = QLabel("—")
            val.setObjectName("mini_status_value")
            stats_grid.addWidget(lbl, row, col * 2)
            stats_grid.addWidget(val, row, col * 2 + 1)
            self._stats[key] = val

        usage_layout.addLayout(stats_grid)
        layout.addWidget(usage_frame)

        # ── Swap status ──────────────────────────────────────
        swap_frame = QFrame()
        swap_frame.setObjectName("card")
        swap_layout = QVBoxLayout(swap_frame)
        swap_layout.setContentsMargins(12, 10, 12, 10)
        swap_layout.setSpacing(6)

        swap_title = QLabel("Swap I/O")
        swap_title.setObjectName("card_title")
        swap_layout.addWidget(swap_title)

        self._swap_stats: dict[str, QLabel] = {}
        swap_grid = QGridLayout()
        swap_grid.setSpacing(4)
        swap_keys = [
            ("swap_count", "Swap ops"),
            ("swap_faults", "Faults"),
            ("swap_failures", "Failures"),
            ("pending_swaps", "Pending"),
            ("waiting_pids", "Waiting PIDs"),
        ]
        for i, (key, label) in enumerate(swap_keys):
            row, col = divmod(i, 3)
            lbl = QLabel(f"{label}:")
            lbl.setObjectName("mini_status_label")
            val = QLabel("—")
            val.setObjectName("mini_status_value")
            swap_grid.addWidget(lbl, row, col * 2)
            swap_grid.addWidget(val, row, col * 2 + 1)
            self._swap_stats[key] = val

        swap_layout.addLayout(swap_grid)
        layout.addWidget(swap_frame)

        # ── Checkpoint / Restore ─────────────────────────────
        ckpt_frame = QFrame()
        ckpt_frame.setObjectName("card")
        ckpt_layout = QVBoxLayout(ckpt_frame)
        ckpt_layout.setContentsMargins(12, 10, 12, 10)
        ckpt_layout.setSpacing(6)

        ckpt_title = QLabel("Checkpoint / Restore")
        ckpt_title.setObjectName("card_title")
        ckpt_layout.addWidget(ckpt_title)

        path_row = QHBoxLayout()
        path_row.addWidget(QLabel("Path (optional):"))
        self.ckpt_path = QLineEdit()
        self.ckpt_path.setPlaceholderText("Leave empty for default")
        path_row.addWidget(self.ckpt_path)
        ckpt_layout.addLayout(path_row)

        ckpt_row = QHBoxLayout()
        ckpt_btn = QPushButton("CHECKPOINT")
        ckpt_btn.setObjectName("primary_button")
        ckpt_btn.setFixedWidth(130)
        ckpt_btn.clicked.connect(lambda: self.checkpoint_requested.emit(self.ckpt_path.text().strip()))
        ckpt_row.addWidget(ckpt_btn)

        restore_btn = QPushButton("RESTORE")
        restore_btn.setFixedWidth(130)
        restore_btn.clicked.connect(lambda: self.restore_requested.emit(self.ckpt_path.text().strip()))
        ckpt_row.addWidget(restore_btn)

        ckpt_row.addStretch()
        ckpt_layout.addLayout(ckpt_row)

        self.ckpt_output = QLabel("")
        self.ckpt_output.setObjectName("card_detail")
        self.ckpt_output.setWordWrap(True)
        ckpt_layout.addWidget(self.ckpt_output)
        layout.addWidget(ckpt_frame)

        # ── MEMW form ────────────────────────────────────────
        memw_frame = QFrame()
        memw_frame.setObjectName("card")
        memw_layout = QVBoxLayout(memw_frame)
        memw_layout.setContentsMargins(12, 10, 12, 10)
        memw_layout.setSpacing(6)

        memw_title = QLabel("Memory Write (MEMW)")
        memw_title.setObjectName("card_title")
        memw_layout.addWidget(memw_title)

        pid_row = QHBoxLayout()
        pid_row.addWidget(QLabel("PID:"))
        self.memw_pid = QLineEdit()
        self.memw_pid.setPlaceholderText("Target PID")
        self.memw_pid.setFixedWidth(80)
        pid_row.addWidget(self.memw_pid)
        pid_row.addStretch()
        memw_layout.addLayout(pid_row)

        self.memw_text = QTextEdit()
        self.memw_text.setPlaceholderText("Raw bytes / text payload to write into NeuralMemory...")
        self.memw_text.setMaximumHeight(80)
        memw_layout.addWidget(self.memw_text)

        send_row = QHBoxLayout()
        send_btn = QPushButton("Send MEMW")
        send_btn.setObjectName("primary_button")
        send_btn.setFixedWidth(120)
        send_btn.clicked.connect(self._on_memw)
        send_row.addStretch()
        send_row.addWidget(send_btn)
        memw_layout.addLayout(send_row)

        layout.addWidget(memw_frame)
        layout.addStretch()

    # ── Public API ───────────────────────────────────────────

    def update_from_status(self, data: dict):
        """Parse global STATUS dict (JSON-decoded) and update memory stats."""
        mem = data.get("memory", {})

        # Memory stats
        for key in self._stats:
            val = mem.get(key, "—")
            self._stats[key].setText(str(val))

        # Usage bar
        total = int(mem.get("total_blocks", 0))
        free = int(mem.get("free_blocks", 0))
        used = total - free
        if total > 0:
            pct = int(used / total * 100)
            self.usage_bar.setValue(pct)
            self.usage_bar.setFormat(f"{used} / {total} blocks ({pct}%)")
        else:
            self.usage_bar.setValue(0)
            self.usage_bar.setFormat("Memory inactive")

        # Swap stats
        for key in self._swap_stats:
            val = mem.get(key, "—")
            self._swap_stats[key].setText(str(val))

    def show_checkpoint_result(self, text: str):
        self.ckpt_output.setText(text[:300])

    def show_memw_result(self, text: str):
        self.ckpt_output.setText(f"MEMW: {text[:300]}")
        QTimer.singleShot(5000, lambda: self.ckpt_output.setText(""))

    def show_error(self, text: str):
        self.error_banner.setText(f"⚠ {text}")
        self.error_banner.setVisible(True)
        QTimer.singleShot(6000, lambda: self.error_banner.setVisible(False))

    # ── Internals ────────────────────────────────────────────

    def _on_memw(self):
        pid = self.memw_pid.text().strip()
        text = self.memw_text.toPlainText().strip()
        if pid and text:
            self.memw_requested.emit(pid, text)
            self.memw_text.clear()


