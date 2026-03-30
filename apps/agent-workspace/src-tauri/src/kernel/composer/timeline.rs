use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::kernel::client::KernelBridge;
use crate::kernel::{history, live_timeline};
use crate::models::kernel::TimelineSnapshot;

use super::lobby::{
    try_fetch_live_snapshot_by_pid, try_fetch_live_snapshot_by_pid_for_session,
    try_fetch_live_snapshot_for_session,
};
use super::workspace::{
    ensure_live_timeline_from_snapshot, snapshot_live_timeline_for_pid,
    snapshot_live_timeline_for_session,
};

pub fn compose_timeline_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<TimelineSnapshot, String> {
    if let Some(timeline) = snapshot_live_timeline_for_session(timeline_store, session_id)? {
        return Ok(timeline);
    }

    let live_snapshot = if let Some(pid) = pid {
        try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, pid)?
    } else {
        try_fetch_live_snapshot_for_session(bridge, session_id)?
    };

    if let Some(snapshot) = live_snapshot {
        ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
        if let Some(timeline) = snapshot_live_timeline_for_session(timeline_store, session_id)? {
            return Ok(timeline);
        }
        return Ok(live_timeline::synthesize_fallback_timeline(snapshot));
    }

    let persisted_workspace = history::load_workspace_snapshot(workspace_root, session_id, pid)?;
    let resolved_pid = pid.or_else(|| {
        persisted_workspace
            .as_ref()
            .and_then(|snapshot| snapshot.active_pid.or(snapshot.last_pid))
    });

    if let Some(timeline) =
        history::load_timeline_snapshot(workspace_root, session_id, resolved_pid)?
    {
        return Ok(timeline);
    }

    Err(format!(
        "No persisted timeline found for session {}",
        session_id
    ))
}

pub fn compose_timeline_snapshot_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<live_timeline::TimelineStore>>,
    pid: u64,
) -> Result<Option<TimelineSnapshot>, String> {
    if let Some(timeline) = snapshot_live_timeline_for_pid(timeline_store, pid)? {
        return Ok(Some(timeline));
    }

    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Ok(None);
    };
    ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;

    if let Some(timeline) = snapshot_live_timeline_for_pid(timeline_store, pid)? {
        return Ok(Some(timeline));
    }

    Ok(Some(live_timeline::synthesize_fallback_timeline(snapshot)))
}
