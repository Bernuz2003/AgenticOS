use std::fs;
use std::path::{Path, PathBuf};

use crate::prompting::PromptFamily;

pub(super) fn normalize_model_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn same_model_path(left: &Path, right: &Path) -> bool {
    normalize_model_path(left) == normalize_model_path(right)
}

pub(super) fn slot_save_dir_for_family(family: PromptFamily) -> PathBuf {
    crate::config::kernel_config()
        .paths
        .workspace_dir
        .join("local-runtimes")
        .join("slots")
        .join(family_key(family))
}

pub(super) fn log_path_for_family(family: PromptFamily) -> PathBuf {
    crate::config::kernel_config()
        .paths
        .workspace_dir
        .join("local-runtimes")
        .join("logs")
        .join(format!("{}.log", family_key(family)))
}

pub(super) fn port_for_family(family: PromptFamily) -> u16 {
    #[cfg(test)]
    let base = super::manager::test_port_base_override_get()
        .unwrap_or(crate::config::kernel_config().external_llamacpp.port_base);
    #[cfg(not(test))]
    let base = crate::config::kernel_config().external_llamacpp.port_base;
    base.saturating_add(match family {
        PromptFamily::Qwen => 0,
        PromptFamily::Llama => 1,
        PromptFamily::Mistral => 2,
        PromptFamily::Unknown => 90,
    })
}

pub(super) fn family_key(family: PromptFamily) -> String {
    family_label(family).to_ascii_lowercase()
}

pub(super) fn family_label(family: PromptFamily) -> &'static str {
    match family {
        PromptFamily::Llama => "Llama",
        PromptFamily::Qwen => "Qwen",
        PromptFamily::Mistral => "Mistral",
        PromptFamily::Unknown => "Unknown",
    }
}

pub(super) fn current_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
