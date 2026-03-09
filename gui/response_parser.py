from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any


_EXEC_PID_RE = re.compile(r"PID:\s*(\d+)")
_PROCESS_FINISHED_RE = re.compile(
    r"\[PROCESS_FINISHED\s+pid=(\d+)\s+tokens_generated=(\d+)\s+elapsed_secs=([0-9.]+)\]"
)


def parse_protocol_envelope(payload: str) -> dict[str, Any] | None:
    try:
        data = json.loads(payload)
    except (json.JSONDecodeError, TypeError):
        return None
    if not isinstance(data, dict):
        return None
    required = {"protocol_version", "schema_id", "request_id", "ok", "code", "data", "error", "warnings"}
    if not required.issubset(data.keys()):
        return None
    return data


def parse_json_payload(payload: str) -> Any:
    try:
        data = json.loads(payload)
    except (json.JSONDecodeError, TypeError):
        return None

    if isinstance(data, dict):
        envelope_data = data.get("data")
        if {
            "protocol_version",
            "schema_id",
            "request_id",
            "ok",
            "code",
            "data",
            "error",
            "warnings",
        }.issubset(data.keys()):
            return envelope_data
    return data


def normalize_control_payload(payload: str, ok: bool) -> str:
    envelope = parse_protocol_envelope(payload)
    if envelope is None:
        return payload

    if ok:
        data = envelope.get("data")
        if isinstance(data, (dict, list)):
            return json.dumps(data)
        if data is None:
            return ""
        return str(data)

    error = envelope.get("error")
    if isinstance(error, dict) and error.get("message"):
        return str(error.get("message"))
    return payload


def parse_json_dict(payload: str) -> dict[str, Any]:
    data = parse_json_payload(payload)
    return data if isinstance(data, dict) else {}


@dataclass
class ProcessDetail:
    pid: str
    workload: str = "—"
    priority: str = "—"
    tokens_generated: str = "—"
    quota_tokens: str = "—"
    syscalls_used: str = "—"
    quota_syscalls: str = "—"
    elapsed: str = "—"


@dataclass
class ProcessRow:
    pid: str
    state: str
    detail: ProcessDetail | None = None


@dataclass
class SchedulerSummary:
    tracked: int = 0
    critical: int = 0
    high: int = 0
    normal: int = 0
    low: int = 0


@dataclass
class StatusSnapshot:
    loaded_model_id: str
    active_pids: list[str]
    uptime_secs: float
    scheduler: SchedulerSummary
    process_rows: list[ProcessRow]
    memory: dict[str, Any]
    raw: dict[str, Any]


@dataclass
class RestoreResult:
    cleared_scheduler_entries: int
    restored_scheduler_entries: int
    selected_model: str
    limitations: list[str]


@dataclass
class ExecStartResult:
    pid: int
    workload: str = ""
    priority: str = ""


@dataclass
class ProcessFinishedMarker:
    pid: int
    tokens_generated: int
    elapsed_secs: float


def parse_models_payload(payload: str) -> tuple[list[dict[str, Any]], dict[str, dict[str, Any]]]:
    data = parse_json_payload(payload)
    if data is None:
        return parse_legacy_model_lines(payload), {}

    if isinstance(data, list):
        models = data
        routing_entries = []
    elif isinstance(data, dict):
        models = data.get("models", []) if isinstance(data.get("models"), list) else []
        routing_entries = (
            data.get("routing_recommendations", [])
            if isinstance(data.get("routing_recommendations"), list)
            else []
        )
    else:
        return parse_legacy_model_lines(payload), {}
    routing = {
        str(entry.get("workload", "")): {
            "model_id": str(entry.get("model_id", "") or ""),
            "source": str(entry.get("source", "") or ""),
            "rationale": str(entry.get("rationale", "") or ""),
            "capability_key": str(entry.get("capability_key", "") or ""),
            "capability_score": entry.get("capability_score"),
        }
        for entry in routing_entries
        if isinstance(entry, dict) and entry.get("workload")
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
                "driver_resolution_source": str(
                    entry.get("driver_resolution_source", "") or ""
                ),
                "driver_resolution_rationale": str(
                    entry.get("driver_resolution_rationale", "") or ""
                ),
                "driver_available": entry.get("driver_available"),
                "driver_load_supported": entry.get("driver_load_supported"),
                "metadata_source": str(entry.get("metadata_source", "") or ""),
                "capabilities": entry.get("capabilities")
                if isinstance(entry.get("capabilities"), dict)
                else None,
            }
        )
    return normalized, routing


def normalize_tool_entry(entry: dict[str, Any]) -> dict[str, Any] | None:
    if not isinstance(entry, dict):
        return None

    descriptor = entry.get("descriptor") if isinstance(entry.get("descriptor"), dict) else {}
    backend = entry.get("backend") if isinstance(entry.get("backend"), dict) else {}
    name = str(descriptor.get("name", "") or "")
    if not name:
        return None

    aliases = descriptor.get("aliases") if isinstance(descriptor.get("aliases"), list) else []
    capabilities = (
        descriptor.get("capabilities")
        if isinstance(descriptor.get("capabilities"), list)
        else []
    )

    return {
        "name": name,
        "aliases": [str(alias) for alias in aliases],
        "description": str(descriptor.get("description", "") or ""),
        "backend_kind": str(descriptor.get("backend_kind", "") or backend.get("kind", "")),
        "backend": backend,
        "source": str(descriptor.get("source", "") or ""),
        "enabled": bool(descriptor.get("enabled", False)),
        "dangerous": bool(descriptor.get("dangerous", False)),
        "capabilities": [str(capability) for capability in capabilities],
        "descriptor": descriptor,
    }


def parse_tools_payload(payload: str) -> list[dict[str, Any]]:
    data = parse_json_payload(payload)
    if not isinstance(data, dict):
        return []

    raw_tools = data.get("tools") if isinstance(data.get("tools"), list) else []
    normalized = []
    for entry in raw_tools:
        if not isinstance(entry, dict):
            continue
        tool = normalize_tool_entry(entry)
        if tool is not None:
            normalized.append(tool)
    return normalized


def parse_tool_info_payload(payload: str) -> dict[str, Any]:
    data = parse_json_dict(payload)
    if not data:
        return {}

    tool = normalize_tool_entry(data.get("tool", {}))
    if tool is None:
        return {}

    sandbox = data.get("sandbox") if isinstance(data.get("sandbox"), dict) else {}
    return {
        "tool": tool,
        "sandbox": sandbox,
    }


def parse_legacy_model_lines(payload: str) -> list[dict[str, Any]]:
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


def normalize_process_detail(data: dict[str, Any]) -> ProcessDetail | None:
    if not isinstance(data, dict):
        return None

    pid = str(data.get("pid", ""))
    if not pid:
        return None

    return ProcessDetail(
        pid=pid,
        workload=str(data.get("workload", "—")),
        priority=str(data.get("priority", "—")),
        tokens_generated=str(data.get("tokens_generated", "—")),
        quota_tokens=str(data.get("quota_tokens", data.get("max_tokens", "—"))),
        syscalls_used=str(data.get("syscalls_used", "—")),
        quota_syscalls=str(data.get("quota_syscalls", data.get("max_syscalls", "—"))),
        elapsed=str(data.get("elapsed_secs", "—")),
    )


def parse_pid_status_payload(payload: str) -> ProcessDetail | None:
    return normalize_process_detail(parse_json_dict(payload))


def parse_status_payload(payload: str) -> StatusSnapshot | None:
    data = parse_json_dict(payload)
    if not data:
        return None

    model = data.get("model", {}) if isinstance(data.get("model"), dict) else {}
    loaded_model_id = str(model.get("loaded_model_id", "") or "")
    processes = data.get("processes", {}) if isinstance(data.get("processes"), dict) else {}
    active_pids = [str(pid) for pid in processes.get("active_pids", [])]
    waiting_pids = [str(pid) for pid in processes.get("waiting_pids", [])]
    in_flight_pids = [str(pid) for pid in processes.get("in_flight_pids", [])]

    detail_by_pid: dict[str, ProcessDetail] = {}
    for proc in processes.get("active_processes", []):
        if not isinstance(proc, dict):
            continue
        detail = normalize_process_detail(proc)
        if detail is not None:
            detail_by_pid[detail.pid] = detail

    rows: list[ProcessRow] = []
    for pid in active_pids:
        rows.append(ProcessRow(pid=pid, state="active", detail=detail_by_pid.get(pid)))
    for pid in waiting_pids:
        rows.append(ProcessRow(pid=pid, state="waiting", detail=detail_by_pid.get(pid)))
    for pid in in_flight_pids:
        rows.append(ProcessRow(pid=pid, state="in-flight", detail=detail_by_pid.get(pid)))

    scheduler_data = data.get("scheduler", {}) if isinstance(data.get("scheduler"), dict) else {}
    memory = data.get("memory", {}) if isinstance(data.get("memory"), dict) else {}

    try:
        uptime_secs = float(data.get("uptime_secs", 0) or 0)
    except (TypeError, ValueError):
        uptime_secs = 0.0

    return StatusSnapshot(
        loaded_model_id=loaded_model_id,
        active_pids=active_pids + in_flight_pids,
        uptime_secs=uptime_secs,
        scheduler=SchedulerSummary(
            tracked=int(scheduler_data.get("tracked", 0) or 0),
            critical=int(scheduler_data.get("priority_critical", 0) or 0),
            high=int(scheduler_data.get("priority_high", 0) or 0),
            normal=int(scheduler_data.get("priority_normal", 0) or 0),
            low=int(scheduler_data.get("priority_low", 0) or 0),
        ),
        process_rows=rows,
        memory=memory,
        raw=data,
    )


def parse_restore_payload(payload: str) -> RestoreResult | None:
    data = parse_json_dict(payload)
    if not data:
        return None

    limitations = data.get("limitations", [])
    return RestoreResult(
        cleared_scheduler_entries=int(data.get("cleared_scheduler_entries", 0) or 0),
        restored_scheduler_entries=int(data.get("restored_scheduler_entries", 0) or 0),
        selected_model=str(data.get("selected_model", "<none>") or "<none>"),
        limitations=[str(item) for item in limitations] if isinstance(limitations, list) else [],
    )


def parse_exec_start_payload(payload: str) -> ExecStartResult | None:
    data = parse_json_dict(payload)
    if data:
        try:
            pid = int(data.get("pid", 0) or 0)
        except (TypeError, ValueError):
            pid = 0
        if pid > 0:
            return ExecStartResult(
                pid=pid,
                workload=str(data.get("workload", "") or ""),
                priority=str(data.get("priority", "") or ""),
            )

    match = _EXEC_PID_RE.search(payload)
    if match is None:
        return None
    return ExecStartResult(pid=int(match.group(1)))


def parse_process_finished_marker(payload: str) -> ProcessFinishedMarker | None:
    match = _PROCESS_FINISHED_RE.search(payload)
    if match is None:
        return None
    return ProcessFinishedMarker(
        pid=int(match.group(1)),
        tokens_generated=int(match.group(2)),
        elapsed_secs=float(match.group(3)),
    )


def split_stream_payload(payload: str) -> tuple[str, ProcessFinishedMarker | None]:
    marker = parse_process_finished_marker(payload)
    if marker is None:
        return payload, None
    cleaned = _PROCESS_FINISHED_RE.sub("", payload).strip()
    return cleaned, marker