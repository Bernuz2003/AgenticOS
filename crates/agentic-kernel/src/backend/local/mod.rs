use std::path::Path;

use anyhow::{Error as E, Result};
use serde_json::json;

use crate::backend::driver_descriptor;
use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::model_catalog::LocalLoadTarget;
use crate::prompting::PromptFamily;

use super::{BackendCapabilities, BackendClass, ContextSlotPersistence, DriverDescriptor};

pub(crate) mod llamacpp;
pub(crate) mod remote_adapter;
pub(crate) mod runtime_manager;

pub(crate) use llamacpp::ExternalLlamaCppBackend;
pub(crate) use runtime_manager::{
    managed_runtime_views, runtime_driver_unavailability_reason,
    shutdown_all as shutdown_managed_runtimes,
};
#[cfg(test)]
pub(crate) use runtime_manager::{
    TestExternalEndpointOverrideGuard, TestExternalRuntimeReadyGuard,
    TestRuntimeDriverAvailabilityGuard,
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

pub(crate) fn diagnose_external_backend() -> Result<serde_json::Value> {
    let endpoint_raw = runtime_manager::diagnostic_endpoint().ok_or_else(|| {
        E::msg(
            "No local runtime is active and no legacy external llama.cpp override is configured; backend diagnostics are unavailable.",
        )
    })?;
    let timeout_ms = std::env::var("AGENTIC_LLAMACPP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(crate::config::kernel_config().external_llamacpp.timeout_ms);
    let endpoint = super::HttpEndpoint::parse(&endpoint_raw)?;
    let backend = ExternalLlamaCppBackend::for_diagnostics(
        endpoint,
        PromptFamily::Unknown,
        timeout_ms,
        crate::config::kernel_config()
            .external_llamacpp
            .chunk_tokens
            .max(1),
    );

    let health = backend.request_json("GET", &backend.endpoint_path("/health"), None);
    let props = backend.request_json("GET", &backend.endpoint_path("/props"), None);
    let slots = backend.request_json("GET", &backend.endpoint_path("/slots"), None);

    fn diag_entry(result: Result<super::HttpJsonResponse>) -> serde_json::Value {
        match result {
            Ok(response) => json!({
                "ok": response.status_code == 200,
                "status_code": response.status_code,
                "status_line": response.status_line,
                "json": response.json,
                "raw_body": response.body,
            }),
            Err(err) => json!({
                "ok": false,
                "error": err.to_string(),
            }),
        }
    }

    let health_entry = diag_entry(health);
    let props_entry = diag_entry(props);
    let slots_entry = diag_entry(slots);

    let props_json = props_entry.get("json");
    let slots_json = slots_entry.get("json").and_then(|value| value.as_array());
    let descriptor = driver_descriptor("external-llamacpp")
        .ok_or_else(|| E::msg("Backend registry is missing external-llamacpp."))?;
    let capabilities = descriptor.capabilities;

    Ok(json!({
        "backend": "external-llamacpp",
        "backend_class": descriptor.class.as_str(),
        "backend_capabilities": {
            "resident_kv": capabilities.resident_kv,
            "persistent_slots": capabilities.persistent_slots,
            "save_restore_slots": capabilities.save_restore_slots,
            "prompt_cache_reuse": capabilities.prompt_cache_reuse,
            "streaming_generation": capabilities.streaming_generation,
            "structured_output": capabilities.structured_output,
            "cancel_generation": capabilities.cancel_generation,
            "memory_telemetry": capabilities.memory_telemetry,
            "tool_pause_resume": capabilities.tool_pause_resume,
            "context_compaction_reset": capabilities.context_compaction_reset,
            "parallel_sessions": capabilities.parallel_sessions,
        },
        "endpoint": endpoint_raw,
        "timeout_ms": timeout_ms,
        "health": health_entry,
        "props": props_entry,
        "slots": slots_entry,
        "summary": {
            "model_path": props_json.and_then(|value| value.get("model_path")).cloned(),
            "total_slots": props_json.and_then(|value| value.get("total_slots")).cloned(),
            "visible_slots": slots_json.map(|slots| slots.len()),
        }
    }))
}

pub(crate) fn backend_for_target(
    target: &LocalLoadTarget,
) -> Result<ExternalLlamaCppBackend, String> {
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
        "external-llamacpp" => persist_external_context_slot_snapshot(family, slot_id, final_path),
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
