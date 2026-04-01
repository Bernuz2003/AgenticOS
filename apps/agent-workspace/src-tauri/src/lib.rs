mod commands;
mod kernel;
mod models;
mod state;
pub mod test_support;
mod utils;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            kernel::events::spawn_event_bridge(
                app.handle().clone(),
                state.kernel_addr.clone(),
                state.workspace_root.clone(),
                state.bridge.clone(),
                state.timeline_store.clone(),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::diagnostics::bootstrap_state,
            commands::diagnostics::fetch_lobby_snapshot,
            commands::workflows::fetch_orchestration_status,
            commands::sessions::fetch_timeline_snapshot,
            commands::sessions::fetch_workspace_snapshot,
            commands::workflows::list_orchestrations,
            commands::models::list_models,
            commands::jobs::list_scheduled_jobs,
            commands::workflows::list_workflow_artifacts,
            commands::models::load_model,
            commands::workflows::orchestrate,
            commands::diagnostics::ping_kernel,
            commands::diagnostics::protocol_preview,
            commands::workflows::retry_workflow_task,
            commands::jobs::schedule_workflow_job,
            commands::jobs::set_scheduled_job_enabled,
            commands::jobs::delete_scheduled_job,
            commands::workflows::stop_workflow_run,
            commands::workflows::delete_workflow_run,
            commands::sessions::continue_session_output,
            commands::sessions::resume_session,
            commands::sessions::send_session_input,
            commands::models::select_model,
            commands::sessions::start_session,
            commands::sessions::stop_session_output,
            commands::diagnostics::shutdown_kernel,
            commands::sessions::delete_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
