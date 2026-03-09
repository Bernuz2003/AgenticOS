from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class GuiSessionState:
    request_retries: int = 2
    retry_delay_s: float = 0.35
    is_connected: bool = False
    last_background_error: str = ""
    status_in_flight: bool = False
    models_in_flight: bool = False
    load_in_flight: bool = False
    active_pids: list[str] = field(default_factory=list)
    loaded_model_id: str = ""
    last_status_payload: str = ""
    default_read_timeout_s: float = 5.0
    default_inactivity_timeout_s: float = 0.5
    load_read_timeout_s: float = 180.0
    load_inactivity_timeout_s: float = 2.0
