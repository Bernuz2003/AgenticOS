use agentic_control_models::{
    LoadModelResult, ModelCatalogSnapshot, OrchestrateResult, SelectModelResult,
};
use tauri::{async_runtime, State};

use crate::app_state::AppState;
use crate::kernel::stream;
use crate::kernel::{audit, auth, protocol};
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
    pid: u64,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge
            .fetch_workspace_snapshot(pid)
            .map_err(|err| err.to_string())
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
        stream::start_exec_session(
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
    pid: u64,
    state: State<'_, AppState>,
) -> Result<TimelineSnapshot, String> {
    let workspace_root = state.workspace_root.clone();
    let timeline_store = state.timeline_store.clone();
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let audit_entries = audit::read_recent_audit_entries_for_pid(&workspace_root, pid, 32);
        if let Some(timeline) = timeline_store
            .lock()
            .map_err(|_| "Timeline store lock poisoned".to_string())?
            .snapshot(pid)
        {
            return Ok(stream::augment_timeline_with_tool_results(
                timeline,
                &audit_entries,
            ));
        }

        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        let snapshot = bridge
            .fetch_workspace_snapshot(pid)
            .map_err(|err| err.to_string())?;
        Ok(stream::augment_timeline_with_tool_results(
            stream::synthesize_fallback_timeline(snapshot),
            &audit_entries,
        ))
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
pub async fn shutdown_kernel(state: State<'_, AppState>) -> Result<String, String> {
    let bridge = state.bridge.clone();
    run_blocking(move || {
        let mut bridge = bridge
            .lock()
            .map_err(|_| "Bridge state lock poisoned".to_string())?;
        bridge.shutdown().map_err(|err| err.to_string())
    })
    .await
}
