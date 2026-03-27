use super::registry::RuntimeRegistry;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeView {
    pub(crate) runtime_id: String,
    pub(crate) target_kind: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) family: String,
    pub(crate) backend_id: String,
    pub(crate) backend_class: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) remote_model_id: Option<String>,
    pub(crate) state: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) pinned: bool,
    pub(crate) transition_state: Option<String>,
    pub(crate) active_pid_count: usize,
    pub(crate) active_pids: Vec<u64>,
    pub(crate) current: bool,
}

impl RuntimeRegistry {
    pub(crate) fn runtime_views(&self) -> Vec<RuntimeView> {
        let mut views = Vec::new();
        for handle in self.runtimes.values() {
            let live_pids = self
                .pid_to_runtime
                .iter()
                .filter_map(|(pid, runtime_id)| {
                    (runtime_id == &handle.descriptor.runtime_id).then_some(*pid)
                })
                .collect::<Vec<_>>();
            views.push(RuntimeView {
                runtime_id: handle.descriptor.runtime_id.clone(),
                target_kind: handle.descriptor.target_kind.clone(),
                logical_model_id: handle.descriptor.logical_model_id.clone(),
                display_path: handle.descriptor.display_path.clone(),
                family: format!("{:?}", handle.descriptor.family),
                backend_id: handle.descriptor.backend_id.clone(),
                backend_class: handle.descriptor.backend_class.as_str().to_string(),
                provider_id: handle.descriptor.provider_id.clone(),
                remote_model_id: handle.descriptor.remote_model_id.clone(),
                state: handle.state.as_str().to_string(),
                reservation_ram_bytes: handle.descriptor.reservation_ram_bytes,
                reservation_vram_bytes: handle.descriptor.reservation_vram_bytes,
                pinned: handle.descriptor.pinned,
                transition_state: handle.descriptor.transition_state.clone(),
                active_pid_count: live_pids.len(),
                active_pids: live_pids,
                current: self.current_runtime_id.as_deref()
                    == Some(handle.descriptor.runtime_id.as_str()),
            });
        }
        views
    }
}
