use std::path::Path;

use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::prompting::PromptFamily;

use super::{BackendCapabilities, BackendClass, ContextSlotPersistence, DriverDescriptor};

pub(crate) mod diagnostics;
pub(crate) mod llamacpp;

pub(crate) use diagnostics::diagnose_external_backend;
pub(crate) use llamacpp::ExternalLlamaCppBackend;

const FAMILIES_COMMON: [PromptFamily; 3] = [
    PromptFamily::Llama,
    PromptFamily::Qwen,
    PromptFamily::Mistral,
];
const ARCH_ANY: [&str; 0] = [];

pub(super) const CAP_EXTERNAL_LLAMACPP: BackendCapabilities = BackendCapabilities {
    resident_kv: true,
    persistent_slots: true,
    save_restore_slots: true,
    prompt_cache_reuse: true,
    streaming_generation: true,
    structured_output: false,
    cancel_generation: false,
    memory_telemetry: true,
    tool_pause_resume: true,
    context_compaction_reset: true,
    parallel_sessions: true,
};

pub(super) const DEFAULT_RESIDENT_BACKEND_ID: &str = "external-llamacpp";

pub(super) const EXTERNAL_LLAMACPP_DRIVER: DriverDescriptor = DriverDescriptor {
    id: "external-llamacpp",
    kind: "resident-adapter",
    class: BackendClass::ResidentLocal,
    capabilities: CAP_EXTERNAL_LLAMACPP,
    available: false,
    load_supported: false,
    note: "Resident local llama.cpp adapter exposed through llama-server.",
    families: &FAMILIES_COMMON,
    architectures: &ARCH_ANY,
};

pub(super) fn endpoint() -> Option<String> {
    #[cfg(test)]
    {
        test_external_endpoint_override_get()
    }

    #[cfg(not(test))]
    {
        let endpoint = crate::config::kernel_config()
            .external_llamacpp
            .endpoint
            .trim()
            .to_string();
        (!endpoint.is_empty()).then_some(endpoint)
    }
}

pub(super) fn runtime_ready() -> bool {
    endpoint().is_some()
}

pub(crate) fn persist_context_slot_payload_for_backend(
    backend_id: &str,
    slot_id: ContextSlotId,
    final_path: &Path,
) -> Result<SlotPersistenceKind, String> {
    match backend_id {
        "external-llamacpp" => persist_external_context_slot_snapshot(slot_id, final_path),
        other => Err(format!(
            "Backend '{}' is not a supported resident inference backend.",
            other
        )),
    }
}

fn persist_external_context_slot_snapshot(
    slot_id: ContextSlotId,
    final_path: &Path,
) -> Result<SlotPersistenceKind, String> {
    let backend =
        ExternalLlamaCppBackend::from_env(PromptFamily::Unknown).map_err(|e| e.to_string())?;
    backend
        .save_context_slot(slot_id, final_path)
        .map_err(|e| e.to_string())?;
    Ok(SlotPersistenceKind::BackendSlotSnapshot)
}

#[cfg(test)]
fn test_external_endpoint_override_get() -> Option<String> {
    let cell = test_external_endpoint_override_cell();
    cell.lock()
        .expect("lock external endpoint override")
        .clone()
}

#[cfg(test)]
fn test_external_endpoint_override_set(value: Option<String>) {
    let cell = test_external_endpoint_override_cell();
    *cell.lock().expect("lock external endpoint override") = value;
}

#[cfg(test)]
fn test_external_endpoint_override_cell() -> &'static std::sync::Mutex<Option<String>> {
    static TEST_EXTERNAL_ENDPOINT_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
        std::sync::OnceLock::new();
    TEST_EXTERNAL_ENDPOINT_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn test_external_endpoint_override_lock() -> &'static std::sync::Mutex<()> {
    static TEST_EXTERNAL_ENDPOINT_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    TEST_EXTERNAL_ENDPOINT_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct TestExternalEndpointOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<String>,
}

#[cfg(test)]
impl TestExternalEndpointOverrideGuard {
    pub(crate) fn set(value: &str) -> Self {
        Self::set_option(Some(value.to_string()))
    }

    pub(crate) fn clear() -> Self {
        Self::set_option(None)
    }

    fn set_option(value: Option<String>) -> Self {
        let lock = test_external_endpoint_override_lock()
            .lock()
            .expect("lock external endpoint override guard");
        let previous = test_external_endpoint_override_get();
        test_external_endpoint_override_set(value);
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestExternalEndpointOverrideGuard {
    fn drop(&mut self) {
        test_external_endpoint_override_set(self.previous.clone());
    }
}
