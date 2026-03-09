from __future__ import annotations

from dataclasses import dataclass


@dataclass
class UiEvent:
    kind: str
    message: str
