use anyhow::{Error as E, Result};
use serde_json::json;

use crate::backend::driver_descriptor;
use crate::prompting::PromptFamily;

use super::llamacpp::ExternalLlamaCppBackend;
use crate::backend::http::{HttpEndpoint, HttpJsonResponse};

pub(crate) fn diagnose_external_backend() -> Result<serde_json::Value> {
    let endpoint_raw = super::runtime_manager::diagnostic_endpoint().ok_or_else(|| {
        E::msg(
            "No local runtime is active and no legacy external llama.cpp override is configured; backend diagnostics are unavailable.",
        )
    })?;
    let timeout_ms = std::env::var("AGENTIC_LLAMACPP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(crate::config::kernel_config().external_llamacpp.timeout_ms);
    let endpoint = HttpEndpoint::parse(&endpoint_raw)?;
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

    fn diag_entry(result: Result<HttpJsonResponse>) -> serde_json::Value {
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
