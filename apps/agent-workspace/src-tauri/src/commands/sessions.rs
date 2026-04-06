use agentic_control_models::{SendInputResult, TurnControlResult};
use tauri::State;

use super::run_blocking;
use crate::kernel::client::transport::KernelBridgeError;
use crate::kernel::composer;
use crate::kernel::{history, live_timeline};
use crate::models::kernel::{
    SessionPathGrantInput, StartSessionResult, TimelineSnapshot, WorkspaceSnapshot,
};
use crate::state::AppState;
use crate::utils::ids::pid_session_fallback;

#[tauri::command]
pub async fn fetch_workspace_snapshot(
    session_id: String,
    pid: Option<u64>,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        composer::compose_workspace_snapshot_for_session(
            &workspace_root,
            &bridge,
            &timeline_store,
            &session_id,
            pid,
        )
    })
    .await
}

#[tauri::command]
pub async fn start_session(
    prompt: String,
    quota_tokens: Option<u64>,
    quota_syscalls: Option<u64>,
    path_grants: Option<Vec<SessionPathGrantInput>>,
    state: State<'_, AppState>,
) -> Result<StartSessionResult, String> {
    let kernel_addr = state.kernel_addr.clone();
    let workspace_root = state.workspace_root.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        live_timeline::start_exec_session(
            kernel_addr,
            workspace_root,
            prompt,
            quota_tokens,
            quota_syscalls,
            path_grants,
            timeline_store,
        )
    })
    .await
}

#[tauri::command]
pub async fn fetch_timeline_snapshot(
    session_id: String,
    pid: Option<u64>,
    state: State<'_, AppState>,
) -> Result<TimelineSnapshot, String> {
    let workspace_root = state.workspace_root.clone();
    let timeline_store = state.timeline_store.clone();
    let bridge = state.bridge.clone();
    run_blocking(move || {
        composer::compose_timeline_snapshot_for_session(
            &workspace_root,
            &bridge,
            &timeline_store,
            &session_id,
            pid,
        )
    })
    .await
}

#[tauri::command]
pub async fn send_session_input(
    pid: Option<u64>,
    session_id: Option<String>,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<SendInputResult, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let result = {
            let mut bridge_guard = bridge
                .lock()
                .map_err(|_| "Bridge state lock poisoned".to_string())?;
            bridge_guard
                .send_input(pid, session_id.as_deref(), &prompt)
                .map_err(|err| err.to_string())?
        };
        let timeline_session_id = session_id
            .clone()
            .unwrap_or_else(|| pid_session_fallback(result.pid));

        if let Err(err) = composer::register_live_user_input(
            &workspace_root,
            &timeline_store,
            &timeline_session_id,
            result.pid,
            None,
            &prompt,
        ) {
            let _ = err;
        }

        Ok(result)
    })
    .await
}

#[tauri::command]
pub async fn continue_session_output(
    pid: u64,
    state: State<'_, AppState>,
) -> Result<TurnControlResult, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        composer::ensure_live_timeline_for_pid(&workspace_root, &bridge, &timeline_store, pid)?;
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        let result = bridge.continue_output(pid).map_err(|err| err.to_string())?;
        if let Ok(mut store) = timeline_store.lock() {
            store.resume_last_turn(pid);
        }
        Ok(result)
    })
    .await
}

#[tauri::command]
pub async fn resume_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<StartSessionResult, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let result = {
            let mut bridge = bridge
                .lock()
                .map_err(|_| "Bridge state lock poisoned".to_string())?;
            bridge
                .resume_session(&session_id)
                .map_err(|err| err.to_string())?
        };

        composer::compose_workspace_snapshot_for_pid(
            &workspace_root,
            &bridge,
            &timeline_store,
            result.pid,
        )?;

        Ok(StartSessionResult {
            session_id: result.session_id,
            pid: result.pid,
        })
    })
    .await
}

#[tauri::command]
pub async fn stop_session_output(
    pid: u64,
    state: State<'_, AppState>,
) -> Result<TurnControlResult, String> {
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        let result = bridge.stop_output(pid).map_err(|err| err.to_string())?;
        if let Ok(mut store) = timeline_store.lock() {
            store.finish_session_with_reason(pid, None, Some("stopped_by_user"));
        }
        Ok(result)
    })
    .await
}

#[tauri::command]
pub async fn delete_session(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let mut live_pids: Vec<u64> = Vec::new();
        if let Ok(mut bridge) = bridge.lock() {
            live_pids = bridge
                .find_live_pids_for_session(&session_id)
                .map_err(|err| {
                    format!(
                        "Failed to resolve live state for session {}: {}",
                        session_id, err
                    )
                })?;
            for pid in &live_pids {
                if let Err(err) = bridge.terminate_pid(*pid) {
                    if !is_pid_already_stopped_error(&err) {
                        return Err(format!("Failed to terminate live PID {}: {}", pid, err));
                    }
                }
            }
        }

        if let Ok(mut store) = timeline_store.lock() {
            for pid in &live_pids {
                store.evict_session(*pid);
            }
            store.evict_session_by_id(&session_id);
        }

        history::delete_session(&workspace_root, &session_id)
    })
    .await
}

fn is_pid_already_stopped_error(err: &KernelBridgeError) -> bool {
    matches!(
        err,
        KernelBridgeError::KernelRejected { code, .. }
            if code == "NO_MODEL" || code == "PID_NOT_FOUND"
    )
}
