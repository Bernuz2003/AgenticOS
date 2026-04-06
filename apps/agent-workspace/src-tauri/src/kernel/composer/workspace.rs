use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::kernel::client::KernelBridge;
use crate::kernel::{history, live_timeline};
use crate::models::kernel::{TimelineSnapshot, WorkspaceSnapshot};

use super::lobby::{
    try_fetch_live_snapshot_by_pid, try_fetch_live_snapshot_by_pid_for_session,
    try_fetch_live_snapshot_for_session,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveTimelineEnsureState {
    ExistingPid,
    ReboundSession,
    SeededHistory,
    InsertedEmpty,
}

pub fn compose_workspace_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<WorkspaceSnapshot, String> {
    if let Some(pid) = pid {
        if let Some(snapshot) = try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, pid)?
        {
            return finalize_workspace_snapshot(workspace_root, timeline_store, snapshot);
        }
        if let Some(snapshot) =
            history::load_workspace_snapshot(workspace_root, session_id, Some(pid))?
        {
            return Ok(snapshot);
        }
    }

    if let Some(snapshot) = try_fetch_live_snapshot_for_session(bridge, session_id)? {
        return finalize_workspace_snapshot(workspace_root, timeline_store, snapshot);
    }

    let Some(persisted) = history::load_workspace_snapshot(workspace_root, session_id, None)?
    else {
        return Err(format!(
            "No persisted workspace snapshot found for session {}",
            session_id
        ));
    };

    if let Some(active_pid) = persisted.active_pid {
        if let Some(snapshot) =
            try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, active_pid)?
        {
            return finalize_workspace_snapshot(workspace_root, timeline_store, snapshot);
        }
    }

    Ok(persisted)
}

pub fn compose_workspace_snapshot_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    pid: u64,
) -> Result<WorkspaceSnapshot, String> {
    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Err(format!("No live workspace snapshot found for PID {}", pid));
    };
    finalize_workspace_snapshot(workspace_root, timeline_store, snapshot)
}

pub fn ensure_live_timeline_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    pid: u64,
) -> Result<String, String> {
    let snapshot = compose_workspace_snapshot_for_pid(workspace_root, bridge, timeline_store, pid)?;
    Ok(snapshot.session_id)
}

pub fn register_live_user_input(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    session_id: &str,
    pid: u64,
    workload_hint: Option<String>,
    prompt: &str,
) -> Result<(), String> {
    let ensure_state = ensure_live_timeline_for_session_pid(
        workspace_root,
        timeline_store,
        session_id,
        pid,
        workload_hint,
    )?;
    let mut store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    if ensure_state == LiveTimelineEnsureState::SeededHistory
        && store.last_turn_matches_pending_user_prompt(pid, prompt)
    {
        return Ok(());
    }
    store.append_user_turn(pid, prompt.to_string());
    Ok(())
}

pub fn ensure_live_timeline_from_snapshot(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    snapshot: WorkspaceSnapshot,
) -> Result<(), String> {
    let _ = ensure_live_timeline_for_session_pid(
        workspace_root,
        timeline_store,
        &snapshot.session_id,
        snapshot.pid,
        Some(snapshot.workload),
    )?;
    Ok(())
}

fn ensure_live_timeline_for_session_pid(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    session_id: &str,
    pid: u64,
    workload_hint: Option<String>,
) -> Result<LiveTimelineEnsureState, String> {
    let workload = workload_hint
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            history::load_workspace_snapshot(workspace_root, session_id, Some(pid))
                .ok()
                .flatten()
                .map(|snapshot| snapshot.workload)
        })
        .unwrap_or_else(|| "general".to_string());

    {
        let mut store = timeline_store
            .lock()
            .map_err(|_| "Timeline store lock poisoned".to_string())?;
        if store.has_pid(pid) {
            return Ok(LiveTimelineEnsureState::ExistingPid);
        }
        if store.has_session_id(session_id) {
            store.rebind_session_pid(session_id, pid, workload);
            return Ok(LiveTimelineEnsureState::ReboundSession);
        }
    }

    let seeded = history::load_timeline_seed(workspace_root, session_id, Some(pid))?;

    let mut store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    if let Some(mut seeded) = seeded {
        seeded.pid = pid;
        if !workload.trim().is_empty() {
            seeded.workload = workload;
        }
        store.insert_seeded_session(seeded);
        Ok(LiveTimelineEnsureState::SeededHistory)
    } else {
        store.insert_empty_session(pid, session_id.to_string(), workload);
        Ok(LiveTimelineEnsureState::InsertedEmpty)
    }
}

pub(super) fn snapshot_live_timeline_for_session(
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    session_id: &str,
) -> Result<Option<TimelineSnapshot>, String> {
    let store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    Ok(store.snapshot_for_session_id(session_id))
}

pub(super) fn snapshot_live_timeline_for_pid(
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    pid: u64,
) -> Result<Option<TimelineSnapshot>, String> {
    let store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    Ok(store.snapshot(pid))
}

fn finalize_workspace_snapshot(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    mut snapshot: WorkspaceSnapshot,
) -> Result<WorkspaceSnapshot, String> {
    ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
    history::hydrate_workspace_snapshot_lineage(workspace_root, &mut snapshot)?;
    history::hydrate_workspace_snapshot_replay(workspace_root, &mut snapshot)?;
    Ok(snapshot)
}
