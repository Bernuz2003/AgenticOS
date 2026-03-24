use std::path::Path;

use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::model_catalog::LocalLoadTarget;
use crate::prompting::PromptFamily;

use super::{BackendCapabilities, BackendClass, ContextSlotPersistence, DriverDescriptor};

pub(crate) mod diagnostics;
pub(crate) mod llamacpp;
pub(crate) mod runtime_manager;

pub(crate) use diagnostics::diagnose_external_backend;
pub(crate) use llamacpp::ExternalLlamaCppBackend;
pub(crate) use runtime_manager::{
    managed_runtime_views, runtime_driver_unavailability_reason,
    shutdown_all as shutdown_managed_runtimes,
};
#[cfg(test)]
pub(crate) use runtime_manager::{
    TestExternalEndpointOverrideGuard, TestExternalRuntimeReadyGuard,
};

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

pub(crate) fn backend_for_target(target: &LocalLoadTarget) -> Result<ExternalLlamaCppBackend, String> {
    let lease = runtime_manager::ensure_runtime_for_target(target)?;
    Ok(ExternalLlamaCppBackend::from_runtime_lease(&lease))
}

pub(crate) fn backend_for_reference(
    reference: &str,
    family: PromptFamily,
) -> Result<ExternalLlamaCppBackend, String> {
    let lease = runtime_manager::ensure_runtime_for_reference(reference, family)?;
    Ok(ExternalLlamaCppBackend::from_runtime_lease(&lease))
}

pub(crate) fn persist_context_slot_payload_for_backend(
    backend_id: &str,
    family: PromptFamily,
    slot_id: ContextSlotId,
    final_path: &Path,
) -> Result<SlotPersistenceKind, String> {
    match backend_id {
        "external-llamacpp" => {
            persist_external_context_slot_snapshot(family, slot_id, final_path)
        }
        other => Err(format!(
            "Backend '{}' is not a supported resident inference backend.",
            other
        )),
    }
}

fn persist_external_context_slot_snapshot(
    family: PromptFamily,
    slot_id: ContextSlotId,
    final_path: &Path,
) -> Result<SlotPersistenceKind, String> {
    let backend = backend_for_reference("", family)?;
    backend
        .save_context_slot(slot_id, final_path)
        .map_err(|e| e.to_string())?;
    Ok(SlotPersistenceKind::BackendSlotSnapshot)
}
