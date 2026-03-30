use super::manager::{
    manager, LocalRuntimeManager, ManagedLocalRuntimeEntry, ManagedLocalRuntimeView,
};
use super::paths::family_label;

impl ManagedLocalRuntimeEntry {
    pub(super) fn view(&self) -> ManagedLocalRuntimeView {
        ManagedLocalRuntimeView {
            family: family_label(self.family).to_string(),
            logical_model_id: self.logical_model_id.clone(),
            display_path: self.model_path.display().to_string(),
            state: self.state.as_str().to_string(),
            endpoint: self.endpoint.clone(),
            port: self.port,
            context_window_tokens: self.context_window_tokens,
            slot_save_dir: self.slot_save_dir.display().to_string(),
            managed_by_kernel: self.managed_by_kernel,
            last_error: self.last_error.clone(),
        }
    }
}

impl LocalRuntimeManager {
    pub(super) fn views(&self) -> Vec<ManagedLocalRuntimeView> {
        self.entries
            .values()
            .map(ManagedLocalRuntimeEntry::view)
            .collect()
    }
}

pub(crate) fn managed_runtime_views() -> Vec<ManagedLocalRuntimeView> {
    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .views()
}
