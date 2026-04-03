use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::kernel::client::KernelBridge;
use crate::kernel::live_timeline::TimelineStore;
pub use crate::models::kernel::{TimelineSnapshot, WorkspaceSnapshot};

pub fn offline_bridge(workspace_root: PathBuf) -> Arc<Mutex<KernelBridge>> {
    Arc::new(Mutex::new(KernelBridge::new(
        "127.0.0.1:9".to_string(),
        workspace_root,
    )))
}

pub fn compose_workspace_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<WorkspaceSnapshot, String> {
    crate::kernel::composer::compose_workspace_snapshot_for_session(
        workspace_root,
        bridge,
        timeline_store,
        session_id,
        pid,
    )
}

pub fn compose_timeline_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<TimelineSnapshot, String> {
    crate::kernel::composer::compose_timeline_snapshot_for_session(
        workspace_root,
        bridge,
        timeline_store,
        session_id,
        pid,
    )
}

pub fn register_live_user_input(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: u64,
    workload_hint: Option<String>,
    prompt: &str,
) -> Result<(), String> {
    crate::kernel::composer::register_live_user_input(
        workspace_root,
        timeline_store,
        session_id,
        pid,
        workload_hint,
        prompt,
    )
}
