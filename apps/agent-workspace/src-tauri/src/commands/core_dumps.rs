use agentic_control_models::{
    CoreDumpInfoResponse, CoreDumpListResponse, CoreDumpReplayResult, CoreDumpSummaryView,
};
use tauri::State;

use super::run_blocking;
use crate::kernel::composer;
use crate::state::AppState;

#[tauri::command]
pub async fn capture_core_dump(
    session_id: Option<String>,
    pid: Option<u64>,
    reason: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
) -> Result<CoreDumpSummaryView, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .capture_core_dump(
                pid,
                session_id.as_deref(),
                reason.as_deref(),
                note.as_deref(),
            )
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_core_dumps(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<CoreDumpListResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.list_core_dumps(limit).map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn fetch_core_dump_info(
    dump_id: String,
    state: State<'_, AppState>,
) -> Result<CoreDumpInfoResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .fetch_core_dump_info(&dump_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn replay_core_dump(
    dump_id: String,
    branch_label: Option<String>,
    state: State<'_, AppState>,
) -> Result<CoreDumpReplayResult, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let result = {
            let mut bridge = bridge
                .lock()
                .map_err(|_| "Bridge state lock poisoned".to_string())?;
            bridge
                .replay_core_dump(&dump_id, branch_label.as_deref())
                .map_err(|err| err.to_string())?
        };

        composer::compose_workspace_snapshot_for_pid(
            &workspace_root,
            &bridge,
            &timeline_store,
            result.pid,
        )?;

        Ok(result)
    })
    .await
}
