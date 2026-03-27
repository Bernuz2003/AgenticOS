use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::prompting::PromptFamily;

use super::manager::{ManagedLocalRuntimeEntry, ManagedRuntimeProcess};
#[cfg(test)]
use super::manager::TestSpawnRequest;
use super::paths::{current_timestamp_ms, family_label, log_path_for_family};

pub(super) fn spawn_llama_server(
    executable: &Path,
    model_path: &Path,
    port: u16,
    context_window_tokens: usize,
    slot_save_dir: &Path,
    family: PromptFamily,
) -> Result<ManagedRuntimeProcess, String> {
    #[cfg(test)]
    if let Some(spawn_hook) = super::manager::test_spawn_hook_get() {
        return spawn_hook(TestSpawnRequest {
            model_path: model_path.to_path_buf(),
            port,
            context_window_tokens,
            slot_save_dir: slot_save_dir.to_path_buf(),
            family,
        });
    }

    let log_path = log_path_for_family(family);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create runtime log directory '{}': {}",
                parent.display(),
                err
            )
        })?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("Failed to open runtime log '{}': {}", log_path.display(), err))?;
    let stderr = stdout
        .try_clone()
        .map_err(|err| format!("Failed to clone runtime log '{}': {}", log_path.display(), err))?;

    let child = Command::new(executable)
        .arg("-m")
        .arg(model_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--slots")
        .arg("--slot-save-path")
        .arg(slot_save_dir)
        .arg("--ctx-size")
        .arg(context_window_tokens.to_string())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|err| {
            format!(
                "Failed to spawn local runtime '{}' for family '{}': {}",
                executable.display(),
                family_label(family),
                err
            )
        })?;

    Ok(ManagedRuntimeProcess::Child(child))
}

pub(super) fn stop_runtime_entry(entry: &mut ManagedLocalRuntimeEntry) {
    if let Some(process) = entry.process.as_mut() {
        process.stop();
    }
    entry.process = None;
    entry.updated_at_ms = current_timestamp_ms();
}

pub(super) fn resolve_llama_server_executable() -> Option<PathBuf> {
    #[cfg(test)]
    if super::manager::test_spawn_hook_get().is_some() {
        return Some(PathBuf::from("test-llama-server"));
    }

    let configured = crate::config::kernel_config()
        .external_llamacpp
        .executable
        .trim()
        .to_string();
    if configured.is_empty() {
        return None;
    }

    let configured_path = PathBuf::from(&configured);
    if configured_path.components().count() > 1 {
        return configured_path.exists().then_some(configured_path);
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&configured);
            candidate.exists().then_some(candidate)
        })
    })
}

pub(super) fn legacy_endpoint_override() -> Option<String> {
    #[cfg(test)]
    if let Some(value) = super::manager::test_external_endpoint_override_get() {
        return Some(value);
    }

    let endpoint = crate::config::kernel_config()
        .external_llamacpp
        .legacy_endpoint_override
        .trim()
        .to_string();
    (!endpoint.is_empty()).then_some(endpoint)
}
