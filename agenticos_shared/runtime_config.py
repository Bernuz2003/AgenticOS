from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import Any

_REPO_ROOT = Path(__file__).resolve().parents[1]
_DEFAULT_BASE_CONFIG = _REPO_ROOT / "config/kernel/base.toml"
_DEFAULT_LEGACY_CONFIG = _REPO_ROOT / "agenticos.toml"
_DEFAULT_LOCAL_CONFIG = _REPO_ROOT / "config/kernel/local.toml"
_DEFAULT_ENV_FILE = _REPO_ROOT / "config/env/agenticos.env"


def _config_paths() -> list[Path]:
    configured = os.environ.get("AGENTIC_CONFIG_PATH")
    local_override = Path(
        os.environ.get("AGENTIC_LOCAL_CONFIG_PATH", str(_DEFAULT_LOCAL_CONFIG))
    )

    if configured:
        paths = [Path(configured)]
    else:
        paths = [_DEFAULT_BASE_CONFIG, _DEFAULT_LEGACY_CONFIG]

    if local_override not in paths:
        paths.append(local_override)
    return paths


def _env_path() -> Path:
    configured = os.environ.get("AGENTIC_ENV_FILE")
    if configured:
        return Path(configured)
    return _DEFAULT_ENV_FILE


def _merge_dict(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    merged = dict(base)
    for key, value in overlay.items():
        current = merged.get(key)
        if isinstance(current, dict) and isinstance(value, dict):
            merged[key] = _merge_dict(current, value)
        else:
            merged[key] = value
    return merged


def _parse_env_value(raw: str) -> str:
    value = raw.strip()
    if len(value) >= 2 and ((value[0] == '"' and value[-1] == '"') or (value[0] == "'" and value[-1] == "'")):
        return value[1:-1]
    return value


def _load_env_file() -> None:
    env_path = _env_path()
    if not env_path.exists():
        return

    try:
        raw = env_path.read_text(encoding="utf-8")
    except OSError:
        return

    for line in raw.splitlines():
        entry = line.strip()
        if not entry or entry.startswith("#"):
            continue
        if entry.startswith("export "):
            entry = entry[len("export ") :].strip()
        if "=" not in entry:
            continue
        key, value = entry.split("=", 1)
        key = key.strip()
        if not key or key in os.environ:
            continue
        os.environ[key] = _parse_env_value(value)


def _load_config() -> dict[str, Any]:
    _load_env_file()

    merged: dict[str, Any] = {}
    for config_path in _config_paths():
        if not config_path.exists():
            continue

        try:
            with config_path.open("rb") as handle:
                data = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError):
            continue

        if isinstance(data, dict):
            merged = _merge_dict(merged, data)

    return merged


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