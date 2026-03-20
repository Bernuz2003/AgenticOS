mod app_state;
mod commands;
mod kernel;
mod models;

use app_state::AppState;
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
            commands::kernel::bootstrap_state,
            commands::kernel::fetch_lobby_snapshot,
            commands::kernel::fetch_orchestration_status,
            commands::kernel::fetch_timeline_snapshot,
            commands::kernel::fetch_workspace_snapshot,
            commands::kernel::list_orchestrations,
            commands::kernel::list_models,
            commands::kernel::list_scheduled_jobs,
            commands::kernel::list_workflow_artifacts,
            commands::kernel::load_model,
            commands::kernel::orchestrate,
            commands::kernel::ping_kernel,
            commands::kernel::protocol_preview,
            commands::kernel::retry_workflow_task,
            commands::kernel::schedule_workflow_job,
            commands::kernel::set_scheduled_job_enabled,
            commands::kernel::delete_scheduled_job,
            commands::kernel::stop_workflow_run,
            commands::kernel::delete_workflow_run,
            commands::kernel::continue_session_output,
            commands::kernel::resume_session,
            commands::kernel::send_session_input,
            commands::kernel::select_model,
            commands::kernel::start_session,
            commands::kernel::stop_session_output,
            commands::kernel::shutdown_kernel,
            commands::kernel::delete_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
