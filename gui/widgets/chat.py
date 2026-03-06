from __future__ import annotations

import html
import re as _re
import time
from dataclasses import dataclass

from PySide6.QtCore import Signal, Qt
from PySide6.QtGui import QKeyEvent
from PySide6.QtWidgets import (
    QComboBox,
    QDoubleSpinBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QTextBrowser,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

# CSS for chat bubbles injected into the QTextBrowser HTML
_CHAT_CSS = """
<style>
body { font-family: 'Segoe UI', 'Ubuntu', sans-serif; font-size: 13px; color: #c0caf5; }
.user-bubble {
    background: #24283b; border: 1px solid #3b4261; border-radius: 12px;
    padding: 10px 14px; margin: 8px 40px 4px 8px;
}
.user-label { color: #7aa2f7; font-weight: 700; font-size: 12px; margin-bottom: 4px; }
.assistant-bubble {
    background: #1f2335; border: 1px solid #3b4261; border-radius: 12px;
    padding: 10px 14px; margin: 4px 8px 8px 40px;
}
.assistant-label { color: #bb9af7; font-weight: 700; font-size: 12px; margin-bottom: 4px; }
.metrics { color: #565f89; font-size: 11px; margin-top: 6px; }
.metrics span { margin-right: 12px; }
pre { background: #16161e; border-radius: 6px; padding: 8px; overflow-x: auto; color: #c0caf5; }
code { color: #9ece6a; }
</style>
"""

_PROCESS_FINISHED_RE = _re.compile(r'\[PROCESS_FINISHED[^\]]*\]')


@dataclass
class _StreamState:
    """Per-PID streaming state."""
    text: str = ""
    start: float = 0.0
    nbytes: int = 0
    bubble_index: int = -1


class ChatSection(QWidget):
    """Chat-style EXEC interface with workload hints and generation params."""

    exec_requested = Signal(str, str)   # (prompt, workload_hint)
    term_requested = Signal(str)        # pid
    kill_requested = Signal(str)        # pid
    set_gen_requested = Signal(str)     # payload
    get_gen_requested = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self._message_count = 0
        self._bubbles: list[str] = []
        self._streams: dict[int, _StreamState] = {}
        self._pending_slots: dict[int, int] = {}
        self._next_req_id = 1

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(8)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Chat")
        title.setObjectName("section_title")
        header.addWidget(title)
        header.addStretch()

        header.addWidget(QLabel("Workload:"))
        self.workload_combo = QComboBox()
        self.workload_combo.addItems(["auto", "fast", "code", "reasoning", "general"])
        self.workload_combo.setFixedWidth(120)
        header.addWidget(self.workload_combo)

        clear_btn = QPushButton("Clear")
        clear_btn.setFixedWidth(60)
        clear_btn.clicked.connect(self._clear_chat)
        header.addWidget(clear_btn)

        layout.addLayout(header)

        # ── Chat display ─────────────────────────────────────
        self.chat_display = QTextBrowser()
        self.chat_display.setOpenExternalLinks(False)
        self.chat_display.setHtml(_CHAT_CSS + "<body></body>")
        layout.addWidget(self.chat_display, stretch=1)

        # ── Prompt input ─────────────────────────────────────
        input_frame = QFrame()
        input_frame.setObjectName("card")
        input_layout = QVBoxLayout(input_frame)
        input_layout.setContentsMargins(8, 8, 8, 8)
        input_layout.setSpacing(6)

        self.prompt_input = QTextEdit()
        self.prompt_input.setPlaceholderText("Scrivi il prompt qui... (Ctrl+Enter per inviare)")
        self.prompt_input.setMaximumHeight(100)
        self.prompt_input.installEventFilter(self)
        input_layout.addWidget(self.prompt_input)

        btn_row = QHBoxLayout()
        self.send_btn = QPushButton("Send")
        self.send_btn.setObjectName("primary_button")
        self.send_btn.setFixedWidth(100)
        self.send_btn.clicked.connect(self._on_send)

        # PID controls
        btn_row.addWidget(QLabel("PID:"))
        self.pid_input = QLineEdit()
        self.pid_input.setPlaceholderText("PID")
        self.pid_input.setFixedWidth(60)
        btn_row.addWidget(self.pid_input)

        term_btn = QPushButton("TERM")
        term_btn.setFixedWidth(60)
        term_btn.clicked.connect(lambda: self.term_requested.emit(self.pid_input.text().strip()))
        btn_row.addWidget(term_btn)

        kill_btn = QPushButton("KILL")
        kill_btn.setObjectName("danger_button")
        kill_btn.setFixedWidth(60)
        kill_btn.clicked.connect(lambda: self.kill_requested.emit(self.pid_input.text().strip()))
        btn_row.addWidget(kill_btn)

        btn_row.addStretch()
        btn_row.addWidget(self.send_btn)
        input_layout.addLayout(btn_row)
        layout.addWidget(input_frame)

        # ── Generation params bar ────────────────────────────
        gen_frame = QFrame()
        gen_frame.setObjectName("card")
        gen_layout = QHBoxLayout(gen_frame)
        gen_layout.setContentsMargins(8, 4, 8, 4)
        gen_layout.setSpacing(12)

        gen_layout.addWidget(QLabel("temp"))
        self.gen_temp = QDoubleSpinBox()
        self.gen_temp.setRange(0.0, 2.0)
        self.gen_temp.setSingleStep(0.05)
        self.gen_temp.setValue(0.6)
        self.gen_temp.setFixedWidth(70)
        gen_layout.addWidget(self.gen_temp)

        gen_layout.addWidget(QLabel("top_p"))
        self.gen_top_p = QDoubleSpinBox()
        self.gen_top_p.setRange(0.0, 1.0)
        self.gen_top_p.setSingleStep(0.05)
        self.gen_top_p.setValue(0.9)
        self.gen_top_p.setFixedWidth(70)
        gen_layout.addWidget(self.gen_top_p)

        gen_layout.addWidget(QLabel("max_tokens"))
        self.gen_max_tokens = QLineEdit("512")
        self.gen_max_tokens.setFixedWidth(60)
        gen_layout.addWidget(self.gen_max_tokens)

        gen_layout.addWidget(QLabel("seed"))
        self.gen_seed = QLineEdit("42")
        self.gen_seed.setFixedWidth(60)
        gen_layout.addWidget(self.gen_seed)

        get_btn = QPushButton("GET")
        get_btn.setFixedWidth(50)
        get_btn.clicked.connect(self.get_gen_requested.emit)
        gen_layout.addWidget(get_btn)

        set_btn = QPushButton("SET")
        set_btn.setFixedWidth(50)
        set_btn.clicked.connect(self._on_set_gen)
        gen_layout.addWidget(set_btn)

        gen_layout.addStretch()
        layout.addWidget(gen_frame)

    # ── Public API (called by MainWindow) ────────────────────

    def begin_user_message(self, prompt: str) -> int:
        """Add user bubble and reserve a slot for the assistant response.
        Returns a request ID to correlate with the kernel PID later."""
        escaped = html.escape(prompt).replace("\n", "<br>")
        bubble = (
            f'<div class="user-bubble">'
            f'<div class="user-label">YOU</div>'
            f'{escaped}</div>'
        )
        self._bubbles.append(bubble)
        req_id = self._next_req_id
        self._next_req_id += 1
        slot_index = len(self._bubbles)
        self._bubbles.append(
            '<div class="assistant-bubble">'
            '<div class="assistant-label" style="color: #565f89;">'
            'ASSISTANT \u25cf waiting...</div></div>'
        )
        self._pending_slots[req_id] = slot_index
        self._message_count += 1
        self._refresh_display()
        return req_id

    def start_assistant_stream(self, pid: int, req_id: int):
        """Associate a kernel PID with the reserved slot and begin streaming."""
        slot_index = self._pending_slots.pop(req_id, None)
        if slot_index is None:
            slot_index = len(self._bubbles)
            self._bubbles.append("")
        state = _StreamState(start=time.perf_counter(), bubble_index=slot_index)
        self._streams[pid] = state
        self._bubbles[slot_index] = self._render_live_bubble(pid)
        self._refresh_display()

    def append_assistant_chunk(self, pid: int, text: str):
        text = _PROCESS_FINISHED_RE.sub('', text)
        if not text:
            return
        state = self._streams.get(pid)
        if state is None:
            return
        state.text += text
        state.nbytes += len(text.encode("utf-8"))
        self._bubbles[state.bubble_index] = self._render_live_bubble(pid)
        self._refresh_display()

    def finish_assistant_message(self, pid: int):
        state = self._streams.pop(pid, None)
        if state is None:
            return
        elapsed = time.perf_counter() - state.start
        escaped = html.escape(state.text).replace("\n", "<br>")
        tok_approx = max(state.nbytes // 4, 1)
        tps = tok_approx / elapsed if elapsed > 0 else 0
        self._bubbles[state.bubble_index] = (
            f'<div class="assistant-bubble">'
            f'<div class="assistant-label">ASSISTANT [PID {pid}]</div>'
            f'{escaped}'
            f'<div class="metrics">'
            f'<span>\u23f1 {elapsed:.1f}s</span>'
            f'<span>\u26a1 {tok_approx} tok</span>'
            f'<span>\U0001f4ca {tps:.1f} tok/s</span>'
            f'</div></div>'
        )
        self._refresh_display()

    def show_error(self, message: str, pid: int = 0, req_id: int = 0):
        error_html = (
            f'<div style="color: #f7768e; padding: 8px; margin: 4px 8px;">'
            f'\u26a0 {html.escape(message)}</div>'
        )
        if pid > 0:
            state = self._streams.pop(pid, None)
            if state is not None:
                self._bubbles[state.bubble_index] = error_html
                self._refresh_display()
                return
        if req_id > 0:
            slot_index = self._pending_slots.pop(req_id, None)
            if slot_index is not None:
                self._bubbles[slot_index] = error_html
                self._refresh_display()
                return
        self._append_html(error_html)

    def apply_generation(self, payload: str):
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

    # ── Internals ────────────────────────────────────────────

    def _render_live_bubble(self, pid: int) -> str:
        state = self._streams.get(pid)
        if state is None:
            return ""
        escaped = html.escape(state.text).replace("\n", "<br>")
        elapsed = time.perf_counter() - state.start
        tok_approx = max(state.nbytes // 4, 1)
        tps = tok_approx / elapsed if elapsed > 0 else 0
        return (
            f'<div class="assistant-bubble">'
            f'<div class="assistant-label">ASSISTANT [PID {pid}] \u25cf streaming...</div>'
            f'{escaped}'
            f'<div class="metrics">'
            f'<span>\u23f1 {elapsed:.1f}s</span>'
            f'<span>\u26a1 {tok_approx} tok</span>'
            f'<span>\U0001f4ca {tps:.1f} tok/s</span>'
            f'</div></div>'
        )

    def _on_send(self):
        prompt = self.prompt_input.toPlainText().strip()
        if not prompt:
            return
        workload = self.workload_combo.currentText()
        self.prompt_input.clear()
        self.exec_requested.emit(prompt, workload)

    def _on_set_gen(self):
        seed = self.gen_seed.text().strip() or "42"
        max_tokens = self.gen_max_tokens.text().strip() or "512"
        payload = (
            f"temperature={self.gen_temp.value():.2f};"
            f"top_p={self.gen_top_p.value():.2f};"
            f"seed={seed};max_tokens={max_tokens}"
        )
        self.set_gen_requested.emit(payload)

    def _append_html(self, html_fragment: str):
        self._bubbles.append(html_fragment)
        self._refresh_display()

    def _refresh_display(self):
        self.chat_display.setHtml(
            _CHAT_CSS + "<body>" + "".join(self._bubbles) + "</body>"
        )
        self.chat_display.verticalScrollBar().setValue(
            self.chat_display.verticalScrollBar().maximum()
        )

    def _clear_chat(self):
        self._bubbles.clear()
        self._streams.clear()
        self._pending_slots.clear()
        self.chat_display.setHtml(_CHAT_CSS + "<body></body>")
        self._message_count = 0

    def eventFilter(self, obj, event):
        if obj is self.prompt_input and isinstance(event, QKeyEvent):
            if event.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter) and event.modifiers() & Qt.KeyboardModifier.ControlModifier:
                self._on_send()
                return True
        return super().eventFilter(obj, event)