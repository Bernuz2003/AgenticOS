use anyhow::{Error as E, Result};
use serde_json::json;

use crate::prompting::PromptFamily;

use super::external_llamacpp::ExternalLlamaCppBackend;
use super::http::{HttpEndpoint, HttpJsonResponse};

pub(crate) fn diagnose_external_backend() -> Result<serde_json::Value> {
    let endpoint_raw = super::external_llamacpp_endpoint().ok_or_else(|| {
        E::msg("AGENTIC_LLAMACPP_ENDPOINT is not configured; external backend diagnostics are unavailable.")
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

    Ok(json!({
        "backend": "external-llamacpp",
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