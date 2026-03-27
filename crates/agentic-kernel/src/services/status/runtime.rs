use agentic_control_models::{
    BackendCapabilitiesView, BackendTelemetryView, ManagedLocalRuntimeView, RuntimeInstanceView,
};

use crate::backend::{managed_runtime_views, BackendCapabilities, RuntimeModel};
use crate::model_catalog::ModelCatalog;

use super::view::StatusSnapshotDeps;

pub(super) fn build_runtime_instances(deps: &StatusSnapshotDeps<'_>) -> Vec<RuntimeInstanceView> {
    deps.runtime_registry
        .runtime_views()
        .into_iter()
        .map(|runtime| RuntimeInstanceView {
            runtime_id: runtime.runtime_id,
            target_kind: runtime.target_kind,
            logical_model_id: runtime.logical_model_id,
            display_path: runtime.display_path,
            family: runtime.family,
            backend_id: runtime.backend_id,
            backend_class: runtime.backend_class,
            provider_id: runtime.provider_id,
            remote_model_id: runtime.remote_model_id,
            state: runtime.state,
            reservation_ram_bytes: runtime.reservation_ram_bytes,
            reservation_vram_bytes: runtime.reservation_vram_bytes,
            pinned: runtime.pinned,
            transition_state: runtime.transition_state,
            active_pid_count: runtime.active_pid_count,
            active_pids: runtime.active_pids,
            current: runtime.current,
        })
        .collect()
}

pub(super) fn build_managed_local_runtimes() -> Vec<ManagedLocalRuntimeView> {
    managed_runtime_views()
        .into_iter()
        .map(|runtime| ManagedLocalRuntimeView {
            family: runtime.family,
            logical_model_id: runtime.logical_model_id,
            display_path: runtime.display_path,
            state: runtime.state,
            endpoint: runtime.endpoint,
            port: runtime.port,
            context_window_tokens: runtime.context_window_tokens,
            slot_save_dir: runtime.slot_save_dir,
            managed_by_kernel: runtime.managed_by_kernel,
            last_error: runtime.last_error,
        })
        .collect()
}

pub(super) fn current_loaded_target_info(
    model_catalog: &ModelCatalog,
    loaded_path: &std::path::Path,
    loaded_remote_model: Option<&agentic_control_models::RemoteModelRuntimeView>,
) -> (String, String, Option<String>, Option<String>) {
    if let Some(model) = loaded_remote_model {
        return (
            model.model_id.clone(),
            "remote_provider".to_string(),
            Some(model.provider_id.clone()),
            Some(model.model_id.clone()),
        );
    }

    let loaded_path = loaded_path.to_string_lossy();
    if let Some(entry) = model_catalog
        .entries
        .iter()
        .find(|entry| entry.path.to_string_lossy() == loaded_path)
    {
        return (entry.id.clone(), "local_catalog".to_string(), None, None);
    }

    (
        loaded_path.to_string(),
        "local_path".to_string(),
        None,
        None,
    )
}

pub(crate) fn runtime_backend_status(
    model: &RuntimeModel,
) -> (
    Option<String>,
    Option<String>,
    Option<BackendCapabilitiesView>,
    Option<BackendTelemetryView>,
) {
    (
        Some(model.backend_id().to_string()),
        Some(model.backend_class().as_str().to_string()),
        Some(model.backend_capabilities().into()),
        model.backend_telemetry(),
    )
}

pub(super) fn map_backend_capabilities(
    capabilities: Option<BackendCapabilities>,
) -> Option<BackendCapabilitiesView> {
    capabilities.map(Into::into)
}
