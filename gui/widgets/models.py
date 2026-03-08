from __future__ import annotations

import json
import re
from pathlib import Path

from PySide6.QtCore import Signal, Qt
from PySide6.QtWidgets import (
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)


class ModelCard(QFrame):
    """Visual card representing one model in the catalog."""

    load_clicked = Signal(str)
    select_clicked = Signal(str)
    info_clicked = Signal(str)

    def __init__(self, model_id: str, parent: QWidget | None = None):
        super().__init__(parent)
        self.model_id = model_id
        self.setObjectName("card")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 10, 12, 10)
        layout.setSpacing(6)

        # Row 1: name + badge
        top_row = QHBoxLayout()
        self.name_label = QLabel(model_id)
        self.name_label.setObjectName("card_title")
        top_row.addWidget(self.name_label)
        top_row.addStretch()

        self.badge = QLabel("available")
        self.badge.setObjectName("badge_available")
        top_row.addWidget(self.badge)
        layout.addLayout(top_row)

        # Row 2: details
        self.detail_label = QLabel("")
        self.detail_label.setObjectName("card_detail")
        self.detail_label.setWordWrap(True)
        layout.addWidget(self.detail_label)

        # Row 3: actions
        btn_row = QHBoxLayout()
        self.load_btn = QPushButton("Load")
        self.load_btn.setObjectName("primary_button")
        self.load_btn.setFixedWidth(80)
        self.load_btn.clicked.connect(lambda: self.load_clicked.emit(self.model_id))

        self.select_btn = QPushButton("Select")
        self.select_btn.setFixedWidth(80)
        self.select_btn.clicked.connect(lambda: self.select_clicked.emit(self.model_id))

        self.info_btn = QPushButton("Info")
        self.info_btn.setFixedWidth(80)
        self.info_btn.clicked.connect(lambda: self.info_clicked.emit(self.model_id))

        btn_row.addWidget(self.load_btn)
        btn_row.addWidget(self.select_btn)
        btn_row.addWidget(self.info_btn)
        btn_row.addStretch()
        layout.addLayout(btn_row)

    def set_state(self, loaded: bool, loading: bool = False):
        if loading:
            self.badge.setText("LOADING...")
            self.badge.setObjectName("badge_loading")
            self.load_btn.setEnabled(False)
        elif loaded:
            self.badge.setText("LOADED")
            self.badge.setObjectName("badge_loaded")
            self.load_btn.setText("Loaded")
            self.load_btn.setEnabled(False)
        else:
            self.badge.setText("available")
            self.badge.setObjectName("badge_available")
            self.load_btn.setText("Load")
            self.load_btn.setEnabled(True)
        self.badge.style().unpolish(self.badge)
        self.badge.style().polish(self.badge)

    def set_details(
        self,
        family: str,
        path: str,
        best_for: str = "",
        architecture: str = "",
        backend: str = "",
        backend_preference: str = "",
        driver_status: str = "",
        metadata_source: str = "",
    ):
        parts = [f"Family: {family}"]
        filename = Path(path).stem if path else ""
        if architecture:
            parts.append(f"Arch: {architecture}")
        if backend:
            parts.append(f"Driver: {backend}")
        if backend_preference and backend_preference != backend:
            parts.append(f"Pref: {backend_preference}")
        if driver_status:
            parts.append(f"Driver state: {driver_status}")
        if filename:
            parts.append(f"File: {filename}")
        if best_for:
            parts.append(f"Best for: {best_for}")
        if metadata_source:
            parts.append(f"Metadata: {metadata_source}")
        self.detail_label.setText("  │  ".join(parts))

    def set_pretty_name(self, name: str):
        self.name_label.setText(name)


# ── Capability routing map ───────────────────────────────────

_WORKLOAD_ORDER = ("fast", "general", "code", "reasoning")


class ModelsSection(QWidget):
    """Model catalog browser with routing map and load/select controls."""

    load_requested = Signal(str)
    select_requested = Signal(str)
    info_requested = Signal(str)
    refresh_requested = Signal()

    def __init__(self, parent: QWidget | None = None):
        super().__init__(parent)
        self._cards: dict[str, ModelCard] = {}
        self._loaded_model_id = ""

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 12)
        layout.setSpacing(12)

        # ── Header ───────────────────────────────────────────
        header = QHBoxLayout()
        title = QLabel("Models")
        title.setObjectName("section_title")
        header.addWidget(title)
        header.addStretch()

        refresh_btn = QPushButton("Refresh")
        refresh_btn.clicked.connect(self.refresh_requested.emit)
        header.addWidget(refresh_btn)
        layout.addLayout(header)

        # ── Model cards scroll area ──────────────────────────
        self._cards_container = QVBoxLayout()
        self._cards_container.setSpacing(8)

        cards_widget = QWidget()
        cards_widget.setLayout(self._cards_container)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(cards_widget)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        layout.addWidget(scroll, stretch=1)

        # ── Routing map ──────────────────────────────────────
        routing_group = QFrame()
        routing_group.setObjectName("card")
        routing_layout = QVBoxLayout(routing_group)
        routing_layout.setContentsMargins(12, 10, 12, 10)

        routing_title = QLabel("Capability Routing Map")
        routing_title.setObjectName("card_title")
        routing_layout.addWidget(routing_title)

        self._routing_grid = QGridLayout()
        self._routing_grid.setSpacing(4)
        routing_layout.addLayout(self._routing_grid)

        self._routing_labels: dict[str, tuple[QLabel, QLabel]] = {}
        for row, workload in enumerate(_WORKLOAD_ORDER):
            wl_label = QLabel(f"  {workload}")
            wl_label.setObjectName("card_detail")
            arrow = QLabel("→")
            arrow.setObjectName("card_detail")
            target = QLabel("—")
            target.setObjectName("mini_status_value")
            source_label = QLabel("(waiting for recommendation)")
            source_label.setObjectName("mini_status_label")
            self._routing_grid.addWidget(wl_label, row, 0)
            self._routing_grid.addWidget(arrow, row, 1)
            self._routing_grid.addWidget(target, row, 2)
            self._routing_grid.addWidget(source_label, row, 3)
            self._routing_labels[workload] = (target, source_label)

        layout.addWidget(routing_group)

        # ── Info output ──────────────────────────────────────
        self.info_output = QLabel("")
        self.info_output.setObjectName("card_detail")
        self.info_output.setWordWrap(True)
        layout.addWidget(self.info_output)

    # ── Public API ───────────────────────────────────────────

    def update_models(self, payload: str):
        """Parse LIST_MODELS JSON response and rebuild cards."""
        self._clear_cards_layout()
        self._cards.clear()

        models, routing = self._parse_models_payload(payload)
        for m in models:
            card = ModelCard(m["id"])
            card.set_pretty_name(self._pretty_name(m["id"], m.get("family", "")))
            card.set_details(
                family=m.get("family", "Unknown"),
                path=m.get("path", ""),
                best_for=self._best_for(m.get("capabilities")),
                architecture=m.get("architecture", ""),
                backend=m.get("resolved_backend", ""),
                backend_preference=m.get("backend_preference", ""),
                driver_status=self._driver_status(m),
                metadata_source=self._metadata_label(m.get("metadata_source", "")),
            )
            card.load_clicked.connect(self._on_load)
            card.select_clicked.connect(self._on_select)
            card.info_clicked.connect(self._on_info)
            self._cards_container.addWidget(card)
            self._cards[m["id"]] = card

        # Spacer at bottom
        self._cards_container.addStretch()

        # Update loaded state
        self._refresh_card_states()
        self._update_routing_map(models, routing)
        if not models:
            self.info_output.setText("LIST_MODELS did not return a recognizable payload.")

    def update_loaded_model(self, model_id: str):
        self._loaded_model_id = model_id
        self._refresh_card_states()

    def show_info(self, text: str):
        try:
            data = json.loads(text)
        except Exception:
            self.info_output.setText(text[:500])
            return

        tokenizer = data.get("tokenizer_path") or "<none>"
        selected = "yes" if data.get("selected") else "no"
        architecture = data.get("architecture") or "unknown"
        backend = data.get("backend_preference") or "auto"
        resolved_backend = data.get("resolved_backend") or "unresolved"
        metadata = self._metadata_label(data.get("metadata_source") or "")
        capabilities = self._best_for(data.get("capabilities")) or "not declared"
        driver_source = data.get("driver_resolution_source") or "unresolved"
        driver_reason = data.get("driver_resolution_rationale") or ""
        driver_status = self._driver_status(data)
        self.info_output.setText(
            f"ID: {data.get('id', '—')}  •  Family: {data.get('family', '—')}  •  Arch: {architecture}  •  Selected: {selected}\n"
            f"Path: {data.get('path', '—')}\n"
            f"Tokenizer: {tokenizer}  •  Driver: {resolved_backend}  •  Pref: {backend}  •  Metadata: {metadata}\n"
            f"Driver source: {driver_source}  •  Driver state: {driver_status}\n"
            f"Capabilities: {capabilities}\n"
            f"Driver rationale: {driver_reason}"
        )

    def set_loading(self, model_id: str):
        if model_id in self._cards:
            self._cards[model_id].set_state(loaded=False, loading=True)

    # ── Internals ────────────────────────────────────────────

    def _on_load(self, model_id: str):
        self.set_loading(model_id)
        self.load_requested.emit(model_id)

    def _on_select(self, model_id: str):
        self.select_requested.emit(model_id)

    def _on_info(self, model_id: str):
        self.info_requested.emit(model_id)

    def _refresh_card_states(self):
        for mid, card in self._cards.items():
            card.set_state(loaded=(mid == self._loaded_model_id))

    def _clear_cards_layout(self):
        while self._cards_container.count():
            item = self._cards_container.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.setParent(None)
                widget.deleteLater()

    def _update_routing_map(self, models: list[dict], routing: dict[str, dict]):
        known_models = {
            m["id"]: self._pretty_name(m["id"], m.get("family", ""))
            for m in models
        }

        for workload in _WORKLOAD_ORDER:
            target_label, source_label = self._routing_labels[workload]
            routed = routing.get(workload, {})
            routed_id = routed.get("model_id", "")
            if routed_id:
                target_label.setText(known_models.get(routed_id, routed_id))
            else:
                target_label.setText("—")

            source = routed.get("source", "")
            rationale = routed.get("rationale", "")
            capability_key = routed.get("capability_key", "")
            capability_score = routed.get("capability_score")
            details = [source] if source else ["no routing hint"]
            if capability_key:
                if isinstance(capability_score, (int, float)):
                    details.append(f"{capability_key}={capability_score:.2f}")
                else:
                    details.append(capability_key)
            if rationale:
                details.append(rationale)
            source_label.setText("  |  ".join(details))

    @staticmethod
    def _parse_models_payload(payload: str) -> tuple[list[dict], dict[str, dict]]:
        try:
            data = json.loads(payload)
        except Exception:
            return ModelsSection._parse_legacy_model_lines(payload), {}

        models = data.get("models", []) if isinstance(data, dict) else []
        routing_entries = data.get("routing_recommendations", []) if isinstance(data, dict) else []
        routing = {
            str(entry.get("workload", "")): {
                "model_id": str(entry.get("model_id", "") or ""),
                "source": str(entry.get("source", "") or ""),
                "rationale": str(entry.get("rationale", "") or ""),
                "capability_key": str(entry.get("capability_key", "") or ""),
                "capability_score": entry.get("capability_score"),
            }
            for entry in routing_entries
            if entry.get("workload")
        }
        normalized = []
        for entry in models:
            if not isinstance(entry, dict) or "id" not in entry:
                continue
            normalized.append(
                {
                    "id": str(entry.get("id", "")),
                    "family": str(entry.get("family", "Unknown")),
                    "architecture": str(entry.get("architecture", "") or ""),
                    "path": str(entry.get("path", "")),
                    "backend_preference": str(entry.get("backend_preference", "") or ""),
                    "resolved_backend": str(entry.get("resolved_backend", "") or ""),
                    "driver_resolution_source": str(entry.get("driver_resolution_source", "") or ""),
                    "driver_resolution_rationale": str(entry.get("driver_resolution_rationale", "") or ""),
                    "driver_available": entry.get("driver_available"),
                    "driver_load_supported": entry.get("driver_load_supported"),
                    "metadata_source": str(entry.get("metadata_source", "") or ""),
                    "capabilities": entry.get("capabilities") if isinstance(entry.get("capabilities"), dict) else None,
                }
            )
        return normalized, routing

    @staticmethod
    def _parse_legacy_model_lines(payload: str) -> list[dict]:
        models = []
        for line in payload.splitlines():
            line = line.strip()
            if "id=" not in line:
                continue
            entry: dict[str, str] = {}
            for key in ("id", "family", "path"):
                match = re.search(rf"{key}=([^\s]+)", line)
                if match:
                    entry[key] = match.group(1)
            if "id" in entry:
                models.append(entry)
        return models

    @staticmethod
    def _pretty_name(model_id: str, family: str) -> str:
        source = model_id.split("/")[-1] if "/" in model_id else model_id
        pretty = source.replace("_", " ").replace("-", " ").strip()
        pretty = re.sub(r"(?i)meta\s*", "", pretty)
        pretty = re.sub(r"(?i)qwen\s*2\.?5", "Qwen 2.5", pretty)
        pretty = re.sub(r"(?i)qwen\s*3\.?5", "Qwen 3.5", pretty)
        pretty = re.sub(r"(?i)llama\s*3\.?1", "Llama 3.1", pretty)
        pretty = re.sub(r"(?i)\b(\d+)b\b", lambda m: f"{m.group(1)}B", pretty)
        pretty = re.sub(r"\s+", " ", pretty).strip()
        if not pretty:
            pretty = model_id
        return f"{pretty} ({family})" if family else pretty

    @staticmethod
    def _best_for(capabilities: dict | None) -> str:
        if not isinstance(capabilities, dict) or not capabilities:
            return ""

        ranked: list[tuple[str, float]] = []
        for key, value in capabilities.items():
            if isinstance(value, (int, float)):
                ranked.append((str(key), float(value)))

        if not ranked:
            return ""

        ranked.sort(key=lambda item: item[1], reverse=True)
        return ", ".join(f"{name} {score:.2f}" for name, score in ranked[:3])

    @staticmethod
    def _metadata_label(metadata_source: str) -> str:
        if not metadata_source:
            return "none"
        return Path(metadata_source).name

    @staticmethod
    def _driver_status(data: dict) -> str:
        available = data.get("driver_available")
        load_supported = data.get("driver_load_supported")
        if available is False or load_supported is False:
            return "stub/unavailable"
        if available is True and load_supported is True:
            return "loadable"
        return "unresolved"
