use agentic_control_models::{ScheduleJobResult, ScheduledJobControlResult, ScheduledJobListResponse};
use tauri::State;

use super::run_blocking;
use crate::state::AppState;

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
