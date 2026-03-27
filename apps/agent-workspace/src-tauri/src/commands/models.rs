use agentic_control_models::{LoadModelResult, ModelCatalogSnapshot, SelectModelResult};
use tauri::State;

use super::run_blocking;
use crate::state::AppState;

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
