from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import Any

_REPO_ROOT = Path(__file__).resolve().parents[1]


def _config_path() -> Path:
    configured = os.environ.get("AGENTIC_CONFIG_PATH")
    if configured:
        return Path(configured)
    return _REPO_ROOT / "agenticos.toml"


def _load_config() -> dict[str, Any]:
    config_path = _config_path()
    if not config_path.exists():
        return {}

    try:
        with config_path.open("rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return {}

    return data if isinstance(data, dict) else {}


def _nested_section(data: dict[str, Any], section_path: tuple[str, ...]) -> dict[str, Any]:
    current: Any = data
    for key in section_path:
        if not isinstance(current, dict):
            return {}
        current = current.get(key, {})
    return current if isinstance(current, dict) else {}


def _coerce_value(value: Any, default: Any) -> Any:
    if isinstance(default, bool):
        return bool(value)
    if isinstance(default, int) and not isinstance(default, bool):
        try:
            return int(value)
        except (TypeError, ValueError):
            return default
    if isinstance(default, float):
        try:
            return float(value)
        except (TypeError, ValueError):
            return default
    if isinstance(default, str):
        return str(value)
    return value


def load_runtime_defaults(defaults: dict[str, Any], *section_path: str) -> dict[str, Any]:
    merged = defaults.copy()
    config = _load_config()

    network = _nested_section(config, ("network",))
    if "host" in merged:
        merged["host"] = str(network.get("host", merged["host"]))
    if "port" in merged:
        merged["port"] = _coerce_value(
            network.get("port", os.environ.get("AGENTIC_PORT", merged["port"])),
            merged["port"],
        )

    section = _nested_section(config, tuple(section_path)) if section_path else {}
    for key, default in defaults.items():
        if key in {"host", "port"}:
            continue
        if key in section:
            merged[key] = _coerce_value(section[key], default)

    return merged