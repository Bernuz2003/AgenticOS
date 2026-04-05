#!/usr/bin/env python3
import argparse
import json
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Read-only inspector for AgenticOS .agentcore.zst dumps"
    )
    parser.add_argument(
        "--workspace-root",
        default=".",
        help="Repository root used to resolve dump ids via workspace/agenticos.db",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="List indexed core dumps")
    list_parser.add_argument("--limit", type=int, default=20)

    info_parser = subparsers.add_parser("info", help="Show dump manifest summary")
    info_parser.add_argument("dump_ref", help="Artifact path or dump_id")

    show_parser = subparsers.add_parser("show", help="Show a focused section of the dump")
    show_parser.add_argument(
        "section",
        choices=["context", "prompt", "turn", "audit", "checkpoints", "invocations"],
    )
    show_parser.add_argument("dump_ref", help="Artifact path or dump_id")

    diff_parser = subparsers.add_parser(
        "diff", help="Compare a replay session against its recorded source baseline"
    )
    diff_parser.add_argument("replay_session_id", help="Replay branch session_id")

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    workspace_root = Path(args.workspace_root).resolve()

    if args.command == "list":
        return cmd_list(workspace_root, args.limit)
    if args.command == "diff":
        return cmd_diff(workspace_root, args.replay_session_id)

    artifact_path = resolve_dump_ref(workspace_root, args.dump_ref)
    manifest = load_manifest(artifact_path)

    if args.command == "info":
        return cmd_info(artifact_path, manifest)
    if args.command == "show":
        return cmd_show(args.section, manifest)
    raise RuntimeError(f"unsupported command: {args.command}")


def cmd_list(workspace_root: Path, limit: int) -> int:
    rows = load_dump_index(workspace_root, limit)
    if not rows:
        print("No core dumps found.")
        return 0

    for row in rows:
        created_at_ms, dump_id, session_id, pid, reason, fidelity, path = row
        session_label = session_id or "-"
        pid_label = str(pid) if pid is not None else "-"
        print(
            f"{created_at_ms}  {dump_id}  pid={pid_label}  session={session_label}  "
            f"reason={reason}  fidelity={fidelity}  path={path}"
        )
    return 0


def cmd_info(artifact_path: Path, manifest: dict[str, Any]) -> int:
    summary = {
        "artifact_path": str(artifact_path),
        "format": manifest.get("format"),
        "dump_id": manifest.get("dump_id"),
        "created_at_ms": manifest.get("created_at_ms"),
        "capture": manifest.get("capture"),
        "target": manifest.get("target"),
        "runtime": manifest.get("runtime"),
        "checkpoint_count": len(manifest.get("debug_checkpoints", [])),
        "tool_invocation_count": len(manifest.get("tool_invocation_history", [])),
        "limitations": manifest.get("limitations", []),
    }
    print(json.dumps(summary, indent=2, ensure_ascii=False))
    return 0


def cmd_show(section: str, manifest: dict[str, Any]) -> int:
    if section == "context":
        payload = {
            "context_policy": deep_get(manifest, "process", "context_policy"),
            "context_state": deep_get(manifest, "process", "context_state"),
        }
    elif section == "prompt":
        payload = {
            "prompt_text": deep_get(manifest, "process", "prompt_text"),
            "rendered_inference_prompt": deep_get(
                manifest, "process", "rendered_inference_prompt"
            ),
            "resident_prompt_suffix": deep_get(
                manifest, "process", "resident_prompt_suffix"
            ),
        }
    elif section == "turn":
        payload = manifest.get("turn_assembly")
    elif section == "audit":
        payload = {
            "session_audit_events": manifest.get("session_audit_events", []),
            "tool_audit_lines": manifest.get("tool_audit_lines", []),
        }
    elif section == "checkpoints":
        payload = manifest.get("debug_checkpoints", [])
    elif section == "invocations":
        payload = manifest.get("tool_invocation_history", [])
    else:
        raise RuntimeError(f"unsupported section: {section}")

    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 0


def cmd_diff(workspace_root: Path, replay_session_id: str) -> int:
    replay_record = load_replay_branch_record(workspace_root, replay_session_id)
    if replay_record is None:
        raise RuntimeError(f"Replay session not found: {replay_session_id}")

    baseline = json.loads(replay_record["baseline_json"])
    current_messages = load_session_messages(workspace_root, replay_session_id)
    current_invocations = load_session_tool_invocations(workspace_root, replay_session_id)
    message_prefix = common_message_prefix_len(
        baseline.get("replay_messages", []),
        current_messages,
    )
    invocation_diffs, changed_tool_outputs, branch_only_tool_calls = build_invocation_diffs(
        baseline.get("tool_invocations", []),
        current_invocations,
    )

    payload = {
        "session_id": replay_session_id,
        "source_dump_id": replay_record["source_dump_id"],
        "source_session_id": replay_record["source_session_id"],
        "source_pid": replay_record["source_pid"],
        "source_fidelity": replay_record["source_fidelity"],
        "replay_mode": replay_record["replay_mode"],
        "tool_mode": replay_record["tool_mode"],
        "initial_state": replay_record["initial_state"],
        "patched_context_segments": replay_record["patched_context_segments"],
        "patched_episodic_segments": replay_record["patched_episodic_segments"],
        "stubbed_invocations": replay_record["stubbed_invocations"],
        "overridden_invocations": replay_record["overridden_invocations"],
        "baseline": {
            "source_context_segments": len(baseline.get("context_segments", [])),
            "source_episodic_segments": len(baseline.get("episodic_segments", [])),
            "source_replay_messages": len(baseline.get("replay_messages", [])),
            "source_tool_invocations": len(baseline.get("tool_invocations", [])),
        },
        "diff": {
            "current_replay_messages": len(current_messages),
            "current_tool_invocations": len(current_invocations),
            "replay_messages_delta": len(current_messages)
            - len(baseline.get("replay_messages", [])),
            "tool_invocations_delta": len(current_invocations)
            - len(baseline.get("tool_invocations", [])),
            "branch_only_messages": max(0, len(current_messages) - message_prefix),
            "branch_only_tool_calls": branch_only_tool_calls,
            "changed_tool_outputs": changed_tool_outputs,
            "completed_tool_calls": sum(
                1 for invocation in current_invocations if invocation["status"] != "dispatched"
            ),
            "latest_branch_message": latest_branch_message(current_messages, message_prefix),
            "invocation_diffs": invocation_diffs[:8],
        },
    }
    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 0


def resolve_dump_ref(workspace_root: Path, dump_ref: str) -> Path:
    candidate = Path(dump_ref)
    if candidate.exists():
        return candidate.resolve()
    path = resolve_dump_id(workspace_root, dump_ref)
    if path is None:
        raise RuntimeError(f"Unknown dump ref: {dump_ref}")
    return path


def resolve_dump_id(workspace_root: Path, dump_id: str) -> Path | None:
    db_path = workspace_root / "workspace" / "agenticos.db"
    if not db_path.exists():
        return None
    query = (
        "SELECT path FROM core_dump_index WHERE dump_id = ?1 "
        "ORDER BY created_at_ms DESC LIMIT 1"
    )
    with sqlite3.connect(db_path) as connection:
        row = connection.execute(query, (dump_id,)).fetchone()
    return Path(row[0]).resolve() if row else None


def load_dump_index(workspace_root: Path, limit: int) -> list[tuple[Any, ...]]:
    db_path = workspace_root / "workspace" / "agenticos.db"
    if not db_path.exists():
        return []
    query = (
        "SELECT created_at_ms, dump_id, session_id, pid, reason, fidelity, path "
        "FROM core_dump_index ORDER BY created_at_ms DESC, dump_id DESC LIMIT ?1"
    )
    with sqlite3.connect(db_path) as connection:
        return connection.execute(query, (max(limit, 1),)).fetchall()


def load_replay_branch_record(
    workspace_root: Path, replay_session_id: str
) -> dict[str, Any] | None:
    db_path = workspace_root / "workspace" / "agenticos.db"
    if not db_path.exists():
        return None
    query = """
        SELECT
            session_id,
            source_dump_id,
            source_session_id,
            source_pid,
            source_fidelity,
            replay_mode,
            tool_mode,
            initial_state,
            patched_context_segments,
            patched_episodic_segments,
            stubbed_invocations,
            overridden_invocations,
            baseline_json
        FROM replay_branch_index
        WHERE session_id = ?1
    """
    with sqlite3.connect(db_path) as connection:
        if not table_exists(connection, "replay_branch_index"):
            return None
        connection.row_factory = sqlite3.Row
        row = connection.execute(query, (replay_session_id,)).fetchone()
    return dict(row) if row else None


def load_session_messages(workspace_root: Path, session_id: str) -> list[dict[str, Any]]:
    db_path = workspace_root / "workspace" / "agenticos.db"
    if not db_path.exists():
        return []
    query = """
        SELECT sm.role, sm.kind, sm.content
        FROM session_messages sm
        JOIN session_turns st ON st.turn_id = sm.turn_id
        WHERE sm.session_id = ?1
        ORDER BY st.turn_index ASC, sm.ordinal ASC, sm.message_id ASC
    """
    with sqlite3.connect(db_path) as connection:
        if not table_exists(connection, "session_messages"):
            return []
        connection.row_factory = sqlite3.Row
        rows = connection.execute(query, (session_id,)).fetchall()
    return [dict(row) for row in rows]


def load_session_tool_invocations(
    workspace_root: Path, session_id: str
) -> list[dict[str, Any]]:
    db_path = workspace_root / "workspace" / "agenticos.db"
    if not db_path.exists():
        return []
    query = """
        SELECT
            invocation_id,
            tool_call_id,
            tool_name,
            command_text,
            status,
            output_text,
            error_kind,
            warnings_json,
            kill
        FROM tool_invocation_history
        WHERE session_id = ?1
        ORDER BY recorded_at_ms ASC, invocation_id ASC
    """
    with sqlite3.connect(db_path) as connection:
        if not table_exists(connection, "tool_invocation_history"):
            return []
        connection.row_factory = sqlite3.Row
        rows = connection.execute(query, (session_id,)).fetchall()
    return [dict(row) for row in rows]


def table_exists(connection: sqlite3.Connection, table_name: str) -> bool:
    query = "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1"
    return connection.execute(query, (table_name,)).fetchone() is not None


def load_manifest(artifact_path: Path) -> dict[str, Any]:
    raw = decode_zstd_file(artifact_path)
    try:
        return json.loads(raw)
    except json.JSONDecodeError as err:
        raise RuntimeError(f"Invalid core dump JSON in {artifact_path}: {err}") from err


def decode_zstd_file(path: Path) -> str:
    zstd_bin = shutil.which("zstd")
    if zstd_bin is not None:
        result = subprocess.run(
            [zstd_bin, "-d", "-c", str(path)],
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            return result.stdout

    try:
        import zstandard  # type: ignore
    except ImportError as err:
        if zstd_bin is None:
            raise RuntimeError(
                "Unable to decode .agentcore.zst: install `zstd` or the `zstandard` Python module."
            ) from err
        raise RuntimeError(
            f"zstd failed to decode '{path}': {result.stderr.strip()}"  # type: ignore[name-defined]
        ) from err

    with path.open("rb") as handle:
        reader = zstandard.ZstdDecompressor().stream_reader(handle)
        data = reader.read()
    return data.decode("utf-8")


def common_message_prefix_len(
    baseline: list[dict[str, Any]], current: list[dict[str, Any]]
) -> int:
    count = 0
    for left, right in zip(baseline, current):
        if (
            left.get("role") != right.get("role")
            or left.get("kind") != right.get("kind")
            or left.get("content") != right.get("content")
        ):
            break
        count += 1
    return count


def latest_branch_message(current: list[dict[str, Any]], prefix_len: int) -> str | None:
    for message in reversed(current[prefix_len:]):
        content = str(message.get("content") or "").strip()
        if content:
            return content
    return None


def build_invocation_diffs(
    baseline: list[dict[str, Any]], current: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], int, int]:
    baseline_by_id = {entry["tool_call_id"]: entry for entry in baseline}
    baseline_by_command: dict[str, list[dict[str, Any]]] = {}
    for entry in baseline:
        baseline_by_command.setdefault(entry["command_text"], []).append(entry)

    matched_source_ids: set[str] = set()
    matched_current_ids: set[int] = set()
    matched_current_by_source: dict[str, dict[str, Any]] = {}

    for invocation in current:
        source_id = replay_stub_source_call_id(invocation)
        if source_id and source_id in baseline_by_id:
            matched_source_ids.add(source_id)
            matched_current_ids.add(int(invocation["invocation_id"]))
            matched_current_by_source[source_id] = invocation

    for invocation in current:
        invocation_id = int(invocation["invocation_id"])
        if invocation_id in matched_current_ids:
            continue
        for candidate in baseline_by_command.get(invocation["command_text"], []):
            if candidate["tool_call_id"] in matched_source_ids:
                continue
            matched_source_ids.add(candidate["tool_call_id"])
            matched_current_ids.add(invocation_id)
            matched_current_by_source[candidate["tool_call_id"]] = invocation
            break

    invocation_diffs: list[dict[str, Any]] = []
    changed_tool_outputs = 0

    for source in baseline:
        replay = matched_current_by_source.get(source["tool_call_id"])
        changed = replay is None or tool_invocation_changed(source, replay)
        if replay is not None and tool_invocation_changed(source, replay):
            changed_tool_outputs += 1
        if changed:
            invocation_diffs.append(
                {
                    "source_tool_call_id": source["tool_call_id"],
                    "replay_tool_call_id": replay["tool_call_id"] if replay else None,
                    "tool_name": source["tool_name"],
                    "command_text": source["command_text"],
                    "source_status": source["status"],
                    "replay_status": replay["status"] if replay else None,
                    "source_output_text": source.get("output_text"),
                    "replay_output_text": replay.get("output_text") if replay else None,
                    "branch_only": False,
                    "changed": True,
                }
            )

    branch_only_tool_calls = 0
    for invocation in current:
        if int(invocation["invocation_id"]) in matched_current_ids:
            continue
        branch_only_tool_calls += 1
        invocation_diffs.append(
            {
                "source_tool_call_id": None,
                "replay_tool_call_id": invocation["tool_call_id"],
                "tool_name": invocation["tool_name"],
                "command_text": invocation["command_text"],
                "source_status": None,
                "replay_status": invocation["status"],
                "source_output_text": None,
                "replay_output_text": invocation.get("output_text"),
                "branch_only": True,
                "changed": True,
            }
        )

    return invocation_diffs, changed_tool_outputs, branch_only_tool_calls


def replay_stub_source_call_id(invocation: dict[str, Any]) -> str | None:
    warnings_json = invocation.get("warnings_json")
    if not warnings_json:
        return None
    try:
        warnings = json.loads(warnings_json)
    except json.JSONDecodeError:
        return None
    if not isinstance(warnings, list):
        return None
    for warning in warnings:
        if not isinstance(warning, str):
            continue
        prefix = "replay_stub_source_call_id="
        if warning.startswith(prefix):
            return warning[len(prefix) :]
    return None


def tool_invocation_changed(source: dict[str, Any], replay: dict[str, Any]) -> bool:
    return any(
        (
            source.get("status") != replay.get("status"),
            source.get("output_text") != replay.get("output_text"),
            source.get("error_kind") != replay.get("error_kind"),
            bool(source.get("kill")) != bool(replay.get("kill")),
        )
    )


def deep_get(payload: dict[str, Any], *keys: str) -> Any:
    current: Any = payload
    for key in keys:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:
        raise SystemExit(141)
    except Exception as err:
        print(f"agent-gdb error: {err}", file=sys.stderr)
        raise SystemExit(1)
