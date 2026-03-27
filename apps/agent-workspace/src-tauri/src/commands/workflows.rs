use agentic_control_models::{
    ArtifactListResponse, OrchStatusResponse, OrchestrateResult, OrchestrationControlResult,
    OrchestrationListResponse, RetryTaskResult,
};
use tauri::State;

use super::run_blocking;
use crate::state::AppState;

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
