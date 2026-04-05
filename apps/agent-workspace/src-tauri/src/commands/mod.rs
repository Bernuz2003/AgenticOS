use tauri::async_runtime;

pub mod core_dumps;
pub mod diagnostics;
pub mod jobs;
pub mod models;
pub mod sessions;
pub mod workflows;

pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    async_runtime::spawn_blocking(task)
        .await
        .map_err(|err| err.to_string())?
}
