use std::sync::{Arc, Mutex};

use crate::kernel::client::KernelBridge;
use crate::models::kernel::WorkspaceSnapshot;

pub(super) fn try_fetch_live_snapshot_for_session(
    bridge: &Arc<Mutex<KernelBridge>>,
    session_id: &str,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let mut bridge = bridge
        .lock()
        .map_err(|_| "Bridge state lock poisoned".to_string())?;
    let live_pid = match bridge.find_live_pid_for_session(session_id) {
        Ok(pid) => pid,
        Err(_) => return Ok(None),
    };
    let Some(live_pid) = live_pid else {
        return Ok(None);
    };

    let Ok(snapshot) = bridge.fetch_workspace_snapshot(live_pid) else {
        return Ok(None);
    };
    if snapshot.session_id != session_id {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub(super) fn try_fetch_live_snapshot_by_pid(
    bridge: &Arc<Mutex<KernelBridge>>,
    pid: u64,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let mut bridge = bridge
        .lock()
        .map_err(|_| "Bridge state lock poisoned".to_string())?;
    let Ok(snapshot) = bridge.fetch_workspace_snapshot(pid) else {
        return Ok(None);
    };
    Ok(Some(snapshot))
}

pub(super) fn try_fetch_live_snapshot_by_pid_for_session(
    bridge: &Arc<Mutex<KernelBridge>>,
    session_id: &str,
    pid: u64,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Ok(None);
    };
    if snapshot.session_id != session_id {
        return Ok(None);
    }
    Ok(Some(snapshot))
}
