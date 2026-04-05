use std::path::Path;

use crate::models::kernel::WorkspaceSnapshot;

use super::audit::{load_accounting_summary, load_audit_events};
use super::db::{load_session_identity, load_turns, open_connection, StoredTurn};
use super::replay::hydrate_workspace_snapshot_replay;

pub fn load_workspace_snapshot(
    workspace_root: &Path,
    session_id: &str,
    pid_hint: Option<u64>,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };
    let Some(identity) = load_session_identity(&connection, session_id)? else {
        return Ok(None);
    };
    let turns = load_turns(&connection, &identity.session_id)?;
    let workload = turns
        .last()
        .map(|turn| turn.workload.clone())
        .unwrap_or(identity.workload);
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| turns.last().map(|turn| turn.pid))
        .or(pid_hint)
        .unwrap_or(0);
    let state = derive_persisted_workspace_state(identity.active_pid, turns.last());
    let accounting = load_accounting_summary(&connection, Some(&identity.session_id))?;
    let audit_events = load_audit_events(&connection, Some(&identity.session_id), 64)?;

    let mut snapshot = WorkspaceSnapshot {
        session_id: identity.session_id,
        pid,
        active_pid: identity.active_pid,
        last_pid: identity.last_pid,
        title: identity.title,
        runtime_id: identity.runtime_id,
        runtime_label: identity.runtime_label,
        state,
        workload,
        owner_id: None,
        tool_caller: None,
        index_pos: None,
        priority: None,
        quota_tokens: None,
        quota_syscalls: None,
        context_slot_id: None,
        resident_slot_policy: None,
        resident_slot_state: None,
        resident_slot_snapshot_path: None,
        backend_id: None,
        backend_class: identity.backend_class,
        backend_capabilities: None,
        accounting,
        permissions: None,
        tokens_generated: 0,
        syscalls_used: 0,
        elapsed_secs: 0.0,
        tokens: 0,
        max_tokens: 0,
        orchestration: None,
        context: None,
        pending_human_request: None,
        audit_events,
        replay: None,
    };
    hydrate_workspace_snapshot_replay(workspace_root, &mut snapshot)?;
    Ok(Some(snapshot))
}

pub(crate) fn derive_persisted_workspace_state(
    active_pid: Option<u64>,
    last_turn: Option<&StoredTurn>,
) -> String {
    if active_pid.is_some() {
        return "Running".to_string();
    }

    let Some(last_turn) = last_turn else {
        return "Idle".to_string();
    };

    match last_turn.status.as_str() {
        "running" => "Running",
        "awaiting_turn_decision" => "AwaitingTurnDecision",
        "errored" => "Errored",
        "killed" => "Killed",
        "terminated" => "Terminated",
        "completed" if last_turn.finish_reason.is_none() => "WaitingForInput",
        "completed" if last_turn.finish_reason.as_deref() == Some("kernel_restarted") => {
            "Interrupted"
        }
        "completed" => "Finished",
        _ => "Idle",
    }
    .to_string()
}
