from __future__ import annotations

import json

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
    QHeaderView,
)

from gui.response_parser import parse_tool_info_payload, parse_tools_payload


class ToolsSection(QWidget):
    refresh_requested = Signal()
    info_requested = Signal(str)
    register_requested = Signal(str)
    unregister_requested = Signal(str)

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self._tools: dict[str, dict] = {}

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(10)

        header = QHBoxLayout()
        title = QLabel("Tools")
        title.setObjectName("section_title")
        header.addWidget(title)
        header.addStretch()

        refresh_btn = QPushButton("Refresh")
        refresh_btn.clicked.connect(self.refresh_requested.emit)
        header.addWidget(refresh_btn)
        layout.addLayout(header)

        self.error_banner = QLabel()
        self.error_banner.setObjectName("error_banner")
        self.error_banner.setWordWrap(True)
        self.error_banner.setVisible(False)
        layout.addWidget(self.error_banner)

        self.table = QTableWidget(0, 5)
        self.table.setHorizontalHeaderLabels(["Name", "Backend", "Source", "Enabled", "Capabilities"])
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.setSelectionMode(QTableWidget.SelectionMode.SingleSelection)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.horizontalHeader().setStretchLastSection(True)
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        self.table.verticalHeader().setVisible(False)
        self.table.currentCellChanged.connect(self._on_row_changed)
        layout.addWidget(self.table, stretch=1)

        detail_card = QFrame()
        detail_card.setObjectName("card")
        detail_layout = QVBoxLayout(detail_card)
        detail_layout.setContentsMargins(12, 10, 12, 10)
        detail_layout.setSpacing(8)

        detail_title = QLabel("Tool Detail")
        detail_title.setObjectName("card_title")
        detail_layout.addWidget(detail_title)

        self.detail_label = QLabel("Select a tool to inspect its registry entry.")
        self.detail_label.setObjectName("card_detail")
        self.detail_label.setWordWrap(True)
        detail_layout.addWidget(self.detail_label)

        action_row = QHBoxLayout()
        info_btn = QPushButton("TOOL_INFO")
        info_btn.clicked.connect(lambda: self.info_requested.emit(self._selected_tool_name()))
        action_row.addWidget(info_btn)

        unregister_btn = QPushButton("UNREGISTER")
        unregister_btn.setObjectName("danger_button")
        unregister_btn.clicked.connect(lambda: self.unregister_requested.emit(self._selected_tool_name()))
        action_row.addWidget(unregister_btn)
        action_row.addStretch()
        detail_layout.addLayout(action_row)

        layout.addWidget(detail_card)

        register_card = QFrame()
        register_card.setObjectName("card")
        register_layout = QVBoxLayout(register_card)
        register_layout.setContentsMargins(12, 10, 12, 10)
        register_layout.setSpacing(8)

        register_title = QLabel("Register / Mutate")
        register_title.setObjectName("card_title")
        register_layout.addWidget(register_title)

        register_help = QLabel(
            "Paste the REGISTER_TOOL JSON payload. The kernel remains the source of truth for validation and privilege checks."
        )
        register_help.setObjectName("card_detail")
        register_help.setWordWrap(True)
        register_layout.addWidget(register_help)

        self.register_editor = QPlainTextEdit()
        self.register_editor.setPlaceholderText(
            '{"descriptor":{"name":"remote_echo","aliases":[],"description":"...","input_schema":{"type":"object"},"output_schema":{"type":"object"},"backend_kind":"remote_http","capabilities":["remote"],"dangerous":false,"enabled":true,"source":"runtime"},"backend":{"kind":"remote_http","url":"http://host:port/invoke","method":"POST","timeout_ms":1000,"headers":{}}}'
        )
        self.register_editor.setMinimumHeight(160)
        register_layout.addWidget(self.register_editor)

        register_btn_row = QHBoxLayout()
        register_btn = QPushButton("REGISTER_TOOL")
        register_btn.setObjectName("primary_button")
        register_btn.clicked.connect(self._on_register_clicked)
        register_btn_row.addWidget(register_btn)

        sample_btn = QPushButton("Load Sample")
        sample_btn.clicked.connect(self._load_sample_payload)
        register_btn_row.addWidget(sample_btn)
        register_btn_row.addStretch()
        register_layout.addLayout(register_btn_row)

        layout.addWidget(register_card)

    def update_tools(self, payload: str):
        self._tools.clear()
        tools = parse_tools_payload(payload)
        self.table.setRowCount(len(tools))

        for row, tool in enumerate(tools):
            name = tool["name"]
            self._tools[name] = tool
            self.table.setItem(row, 0, QTableWidgetItem(name))
            self.table.setItem(row, 1, QTableWidgetItem(tool.get("backend_kind", "—")))
            self.table.setItem(row, 2, QTableWidgetItem(tool.get("source", "—")))
            self.table.setItem(row, 3, QTableWidgetItem("yes" if tool.get("enabled") else "no"))
            self.table.setItem(row, 4, QTableWidgetItem(", ".join(tool.get("capabilities", [])) or "—"))

        if tools:
            self.table.setCurrentCell(0, 0)
            self._render_tool_detail(tools[0], {})
        else:
            self.detail_label.setText("LIST_TOOLS did not return any recognizable registry entries.")

    def clear_tools(self):
        self._tools.clear()
        self.table.setRowCount(0)
        self.detail_label.setText("No tool registry data loaded.")
        self.clear_error()

    def show_tool_info(self, payload: str):
        info = parse_tool_info_payload(payload)
        if not info:
            self.detail_label.setText(payload[:1200])
            return
        self._render_tool_detail(info["tool"], info.get("sandbox", {}))

    def show_tool_mutation_result(self, payload: str, action: str):
        data = json.loads(payload) if payload.strip() else {}
        tool = None
        if isinstance(data, dict):
            raw_tool = data.get("tool")
            if isinstance(raw_tool, dict):
                normalized = parse_tool_info_payload(json.dumps({"tool": raw_tool, "sandbox": {}}))
                tool = normalized.get("tool") if normalized else None
        if tool is None:
            self.detail_label.setText(f"{action} succeeded.\n\n{payload[:1200]}")
            return
        self._render_tool_detail(tool, {})

    def show_error(self, text: str):
        self.error_banner.setText(f"⚠ {text}")
        self.error_banner.setVisible(True)

    def clear_error(self):
        self.error_banner.setVisible(False)

    def _selected_tool_name(self) -> str:
        row = self.table.currentRow()
        if row < 0:
            return ""
        item = self.table.item(row, 0)
        return item.text() if item else ""

    def _on_row_changed(self, row: int, _col: int, _prev_row: int, _prev_col: int):
        if row < 0:
            return
        tool_name = self._selected_tool_name()
        if tool_name:
            tool = self._tools.get(tool_name)
            if tool:
                self._render_tool_detail(tool, {})

    def _on_register_clicked(self):
        payload = self.register_editor.toPlainText().strip()
        if payload:
            self.register_requested.emit(payload)

    def _load_sample_payload(self):
        self.register_editor.setPlainText(
            json.dumps(
                {
                    "descriptor": {
                        "name": "remote_echo",
                        "aliases": [],
                        "description": "Forward payload to a remote HTTP endpoint.",
                        "input_schema": {"type": "object"},
                        "output_schema": {
                            "type": "object",
                            "required": ["output"],
                            "properties": {"output": {"type": "string"}},
                            "additionalProperties": False,
                        },
                        "backend_kind": "remote_http",
                        "capabilities": ["remote"],
                        "dangerous": False,
                        "enabled": True,
                        "source": "runtime",
                    },
                    "backend": {
                        "kind": "remote_http",
                        "url": "http://host:port/invoke",
                        "method": "POST",
                        "timeout_ms": 1000,
                        "headers": {},
                    },
                },
                indent=2,
            )
        )

    def _render_tool_detail(self, tool: dict, sandbox: dict):
        aliases = ", ".join(tool.get("aliases", [])) or "—"
        backend = tool.get("backend", {}) if isinstance(tool.get("backend"), dict) else {}
        backend_json = json.dumps(backend, indent=2, ensure_ascii=True)
        sandbox_json = json.dumps(sandbox, indent=2, ensure_ascii=True) if sandbox else "{}"
        self.detail_label.setText(
            f"Name: {tool.get('name', '—')}  │  Backend: {tool.get('backend_kind', '—')}  │  Source: {tool.get('source', '—')}\n"
            f"Enabled: {'yes' if tool.get('enabled') else 'no'}  │  Dangerous: {'yes' if tool.get('dangerous') else 'no'}\n"
            f"Aliases: {aliases}\n"
            f"Capabilities: {', '.join(tool.get('capabilities', [])) or '—'}\n"
            f"Description: {tool.get('description', '—')}\n\n"
            f"Backend Config:\n{backend_json}\n\n"
            f"Sandbox:\n{sandbox_json}"
        )