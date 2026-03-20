use agentic_control_models::{
    ArtifactListResponse, LoadModelResult, ModelCatalogSnapshot, OrchStatusResponse,
    OrchestrateResult, OrchestrationControlResult, OrchestrationListResponse, RetryTaskResult,
    ScheduleJobResult, ScheduledJobControlResult, ScheduledJobListResponse, SelectModelResult,
    SendInputResult, TurnControlResult,
};
use tauri::{async_runtime, State};

use crate::app_state::AppState;
use crate::kernel::composer;
use crate::kernel::error::KernelBridgeError;
use crate::kernel::live_cache;
use crate::kernel::persisted_truth;
use crate::kernel::{auth, protocol};
use crate::models::kernel::{
    KernelBootstrapState, LobbySnapshot, StartSessionResult, TimelineSnapshot, WorkspaceSnapshot,
};

async fn run_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn bootstrap_state(state: State<'_, AppState>) -> KernelBootstrapState {
    KernelBootstrapState {
        kernel_addr: state.kernel_addr.clone(),
        workspace_root: state.workspace_root.display().to_string(),
        protocol_version: protocol::default_protocol_version().to_string(),
        connection_mode: "tcp-authenticated-bridge".to_string(),
    }
}

#[tauri::command]
pub fn protocol_preview(state: State<'_, AppState>) -> String {
    let token_path = auth::kernel_token_path(&state.workspace_root);
    format!(
        "protocol={} token_path={}",
        protocol::default_protocol_version(),
        token_path.display()
    )
}

#[tauri::command]
pub async fn fetch_lobby_snapshot(state: State<'_, AppState>) -> Result<LobbySnapshot, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        Ok(bridge.fetch_lobby_snapshot())
    })
    .await
}

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
    workload: String,
    state: State<'_, AppState>,
) -> Result<StartSessionResult, String> {
    let kernel_addr = state.kernel_addr.clone();
    let workspace_root = state.workspace_root.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        live_cache::start_exec_session(
            kernel_addr,
            workspace_root,
            prompt,
            workload,
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
pub async fn orchestrate(
    payload: String,
    state: State<'_, AppState>,
) -> Result<OrchestrateResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.orchestrate(&payload).map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn schedule_workflow_job(
    payload: String,
    state: State<'_, AppState>,
) -> Result<ScheduleJobResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.schedule_job(&payload).map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn fetch_orchestration_status(
    orchestration_id: u64,
    state: State<'_, AppState>,
) -> Result<OrchStatusResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .fetch_orchestration_status(orchestration_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_orchestrations(
    state: State<'_, AppState>,
) -> Result<OrchestrationListResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.list_orchestrations().map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_scheduled_jobs(
    state: State<'_, AppState>,
) -> Result<ScheduledJobListResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.list_scheduled_jobs().map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_workflow_artifacts(
    orchestration_id: u64,
    task: Option<String>,
    state: State<'_, AppState>,
) -> Result<ArtifactListResponse, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .list_artifacts(orchestration_id, task.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn retry_workflow_task(
    orchestration_id: u64,
    task_id: String,
    state: State<'_, AppState>,
) -> Result<RetryTaskResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .retry_task(orchestration_id, &task_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn stop_workflow_run(
    orchestration_id: u64,
    state: State<'_, AppState>,
) -> Result<OrchestrationControlResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .stop_orchestration(orchestration_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_workflow_run(
    orchestration_id: u64,
    state: State<'_, AppState>,
) -> Result<OrchestrationControlResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .delete_orchestration(orchestration_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn set_scheduled_job_enabled(
    job_id: u64,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<ScheduledJobControlResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .set_job_enabled(job_id, enabled)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_scheduled_job(
    job_id: u64,
    state: State<'_, AppState>,
) -> Result<ScheduledJobControlResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.delete_job(job_id).map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn ping_kernel(state: State<'_, AppState>) -> Result<String, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.ping().map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<ModelCatalogSnapshot, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.list_models().map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn select_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<SelectModelResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .select_model(&model_id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn load_model(
    selector: String,
    state: State<'_, AppState>,
) -> Result<LoadModelResult, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.load_model(&selector).map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn send_session_input(
    pid: u64,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<SendInputResult, String> {
    let workspace_root = state.workspace_root.clone();
    let bridge = state.bridge.clone();
    let timeline_store = state.timeline_store.clone();
    run_blocking(move || {
        let session_id =
            composer::ensure_live_timeline_for_pid(&workspace_root, &bridge, &timeline_store, pid)?;
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        let result = bridge
            .send_input(pid, &prompt)
            .map_err(|err| err.to_string())?;

        if let Ok(mut store) = timeline_store.lock() {
            if !store.has_pid(pid) {
                store.insert_empty_session(pid, session_id, "general".to_string());
            }
            store.append_user_turn(pid, prompt);
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
pub async fn shutdown_kernel(state: State<'_, AppState>) -> Result<String, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        match bridge.shutdown() {
            Ok(message) => Ok(message),
            Err(err) if is_expected_shutdown_disconnect(&err) => {
                Ok("Kernel shutdown requested".to_string())
            }
            Err(err) => Err(err.to_string()),
        }
    })
    .await
}

fn is_expected_shutdown_disconnect(err: &KernelBridgeError) -> bool {
    match err {
        KernelBridgeError::ConnectionClosed | KernelBridgeError::ConnectionUnavailable => true,
        KernelBridgeError::Io(io_err) => matches!(
            io_err.kind(),
            std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::UnexpectedEof
        ),
        _ => false,
    }
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

        persisted_truth::delete_session(&workspace_root, &session_id)
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
