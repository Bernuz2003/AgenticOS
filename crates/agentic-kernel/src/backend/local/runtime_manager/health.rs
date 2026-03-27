use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::backend::HttpEndpoint;

use super::manager::{manager, ManagedLocalRuntimeEntry, ManagedLocalRuntimeState};
use super::paths::{current_timestamp_ms, family_label, same_model_path};
use super::spawn::{legacy_endpoint_override, resolve_llama_server_executable};

pub(crate) fn diagnostic_endpoint() -> Option<String> {
    if let Some(endpoint) = legacy_endpoint_override() {
        return Some(endpoint);
    }

    manager()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entries
        .values()
        .next()
        .map(|entry| entry.endpoint.clone())
}

pub(crate) fn runtime_driver_available() -> bool {
    runtime_driver_unavailability_reason().is_none()
}

pub(crate) fn runtime_driver_unavailability_reason() -> Option<String> {
    #[cfg(test)]
    if let Some(override_reason) = super::manager::test_driver_unavailability_override_get() {
        return override_reason;
    }

    if legacy_endpoint_override().is_some() {
        return None;
    }

    let configured = crate::config::kernel_config()
        .external_llamacpp
        .executable
        .trim()
        .to_string();
    if configured.is_empty() {
        return Some(
            "Local runtime backend is unavailable: [external_llamacpp].executable is empty and no legacy endpoint override is configured."
                .to_string(),
        );
    }

    let configured_path = PathBuf::from(&configured);
    if configured_path.components().count() > 1 {
        if configured_path.exists() {
            return None;
        }
        return Some(format!(
            "Local runtime backend is unavailable: configured llama-server executable '{}' does not exist. Set [external_llamacpp].executable in config/kernel/local.toml or AGENTIC_LLAMACPP_EXECUTABLE to the real binary path.",
            configured_path.display()
        ));
    }

    if resolve_llama_server_executable().is_some() {
        return None;
    }

    Some(format!(
        "Local runtime backend is unavailable: executable '{}' was not found on PATH. Install llama.cpp's llama-server or set [external_llamacpp].executable in config/kernel/local.toml (or AGENTIC_LLAMACPP_EXECUTABLE) to its absolute path.",
        configured
    ))
}

pub(super) fn wait_until_runtime_ready(
    entry: &mut ManagedLocalRuntimeEntry,
    expected_model_path: &Path,
) -> Result<(), String> {
    let timeout = Duration::from_millis(
        crate::config::kernel_config()
            .external_llamacpp
            .startup_timeout_ms
            .max(1),
    );
    let poll = Duration::from_millis(
        crate::config::kernel_config()
            .external_llamacpp
            .health_poll_ms
            .max(10),
    );
    let start = Instant::now();

    loop {
        if let Some(process) = entry.process.as_mut() {
            if let Some(status) = process.try_wait()? {
                return Err(format!(
                    "Local runtime '{}' exited before becoming healthy (status: {}).",
                    family_label(entry.family),
                    status
                ));
            }
        }

        match probe_runtime(&entry.endpoint, expected_model_path) {
            Ok(()) => {
                entry.state = if entry.managed_by_kernel {
                    ManagedLocalRuntimeState::Ready
                } else {
                    ManagedLocalRuntimeState::ExternalOverride
                };
                entry.last_error = None;
                entry.updated_at_ms = current_timestamp_ms();
                return Ok(());
            }
            Err(err) if start.elapsed() < timeout => {
                entry.last_error = Some(err);
                std::thread::sleep(poll);
            }
            Err(err) => {
                return Err(format!(
                    "Local runtime '{}' failed to become healthy within {} ms: {}",
                    family_label(entry.family),
                    timeout.as_millis(),
                    err
                ));
            }
        }
    }
}

pub(super) fn probe_runtime(endpoint: &str, expected_model_path: &Path) -> Result<(), String> {
    let endpoint = HttpEndpoint::parse(endpoint).map_err(|err| err.to_string())?;
    let timeout_ms = crate::config::kernel_config().external_llamacpp.timeout_ms;
    let health = endpoint
        .request_json("GET", &endpoint.joined_path("/health"), None, timeout_ms)
        .map_err(|err| format!("health check failed: {}", err))?;
    if health.status_code != 200 {
        return Err(format!("health returned {}", health.status_line));
    }

    let props = endpoint
        .request_json("GET", &endpoint.joined_path("/props"), None, timeout_ms)
        .map_err(|err| format!("props check failed: {}", err))?;
    if props.status_code != 200 {
        return Err(format!("props returned {}", props.status_line));
    }

    let reported_model = props
        .json
        .as_ref()
        .and_then(|value| value.get("model_path"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| "props response is missing model_path".to_string())?;
    if !same_model_path(Path::new(reported_model), expected_model_path) {
        return Err(format!(
            "runtime is serving '{}' instead of '{}'",
            reported_model,
            expected_model_path.display()
        ));
    }

    Ok(())
}
