use agentic_control_models::BackendTelemetryView;

use super::{BackendCapabilities, BackendClass, DriverDescriptor};
use crate::prompting::PromptFamily;

pub(crate) mod groq;
pub(crate) mod openai_compatible;
pub(crate) mod openrouter;
pub(crate) mod streaming;

pub(crate) use openai_compatible::RemoteOpenAICompatibleBackend;

const FAMILIES_ALL: [PromptFamily; 4] = [
    PromptFamily::Llama,
    PromptFamily::Qwen,
    PromptFamily::Mistral,
    PromptFamily::Unknown,
];
const ARCH_ANY: [&str; 0] = [];

pub(super) const CAP_REMOTE_OPENAI_COMPATIBLE: BackendCapabilities = BackendCapabilities {
    resident_kv: false,
    persistent_slots: false,
    save_restore_slots: false,
    prompt_cache_reuse: false,
    streaming_generation: true,
    structured_output: true,
    cancel_generation: false,
    memory_telemetry: false,
    tool_pause_resume: false,
    context_compaction_reset: false,
    parallel_sessions: true,
};

pub(super) const OPENAI_RESPONSES_DRIVER: DriverDescriptor = DriverDescriptor {
    id: "openai-responses",
    kind: "remote-api",
    class: BackendClass::RemoteStateless,
    capabilities: CAP_REMOTE_OPENAI_COMPATIBLE,
    available: false,
    load_supported: false,
    note: "Remote stateless OpenAI Responses API backend.",
    families: &FAMILIES_ALL,
    architectures: &ARCH_ANY,
};

pub(super) const GROQ_RESPONSES_DRIVER: DriverDescriptor = DriverDescriptor {
    id: "groq-responses",
    kind: "remote-api",
    class: BackendClass::RemoteStateless,
    capabilities: CAP_REMOTE_OPENAI_COMPATIBLE,
    available: false,
    load_supported: false,
    note: "Remote stateless Groq backend via OpenAI-compatible Responses API.",
    families: &FAMILIES_ALL,
    architectures: &ARCH_ANY,
};

pub(super) const OPENROUTER_DRIVER: DriverDescriptor = DriverDescriptor {
    id: "openrouter",
    kind: "remote-api",
    class: BackendClass::RemoteStateless,
    capabilities: CAP_REMOTE_OPENAI_COMPATIBLE,
    available: false,
    load_supported: false,
    note: "Remote stateless OpenRouter backend via OpenAI-compatible chat completions.",
    families: &FAMILIES_ALL,
    architectures: &ARCH_ANY,
};

pub(crate) fn runtime_backend_telemetry(backend_id: &str) -> Option<BackendTelemetryView> {
    match backend_id {
        "openai-responses" | "groq-responses" | "openrouter" => {
            openai_compatible::telemetry_snapshot(backend_id)
        }
        _ => None,
    }
}

pub(crate) fn runtime_config(
    backend_id: &str,
) -> Option<crate::config::RemoteProviderRuntimeConfig> {
    #[cfg(test)]
    {
        if let Some(config) = test_remote_openai_config_override_get(backend_id) {
            return Some(config);
        }
    }

    let config = crate::config::kernel_config();
    match backend_id {
        "openai-responses" => Some(config.openai_responses.clone().into()),
        "groq-responses" => Some(config.groq_responses.clone().into()),
        "openrouter" => Some(config.openrouter.clone().into()),
        _ => None,
    }
}

pub(super) fn runtime_ready(backend_id: &str) -> bool {
    runtime_config(backend_id).is_some_and(|config| {
        !config.endpoint.trim().is_empty() && !config.api_key.trim().is_empty()
    })
}

#[cfg(test)]
fn test_remote_openai_config_override_get(
    backend_id: &str,
) -> Option<crate::config::RemoteProviderRuntimeConfig> {
    let cell = test_remote_openai_config_override_cell();
    cell.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(backend_id)
        .cloned()
}

#[cfg(test)]
fn test_remote_openai_config_override_set(
    backend_id: &str,
    value: Option<crate::config::RemoteProviderRuntimeConfig>,
) {
    let cell = test_remote_openai_config_override_cell();
    let mut overrides = cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = value {
        overrides.insert(backend_id.to_string(), value);
    } else {
        overrides.remove(backend_id);
    }
}

#[cfg(test)]
fn test_remote_openai_config_override_cell() -> &'static std::sync::Mutex<
    std::collections::BTreeMap<String, crate::config::RemoteProviderRuntimeConfig>,
> {
    static TEST_REMOTE_OPENAI_CONFIG_OVERRIDE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::BTreeMap<String, crate::config::RemoteProviderRuntimeConfig>,
        >,
    > = std::sync::OnceLock::new();
    TEST_REMOTE_OPENAI_CONFIG_OVERRIDE
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
}

#[cfg(test)]
fn test_remote_openai_config_override_lock() -> &'static std::sync::Mutex<()> {
    static TEST_OPENAI_CONFIG_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
        std::sync::OnceLock::new();
    TEST_OPENAI_CONFIG_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct TestRemoteOpenAIConfigOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    backend_id: String,
    previous: Option<crate::config::RemoteProviderRuntimeConfig>,
}

#[cfg(test)]
impl TestRemoteOpenAIConfigOverrideGuard {
    pub(crate) fn set(
        backend_id: &str,
        config: crate::config::RemoteProviderRuntimeConfig,
    ) -> Self {
        let lock = test_remote_openai_config_override_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = test_remote_openai_config_override_get(backend_id);
        test_remote_openai_config_override_set(backend_id, Some(config));
        Self {
            _lock: lock,
            backend_id: backend_id.to_string(),
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for TestRemoteOpenAIConfigOverrideGuard {
    fn drop(&mut self) {
        test_remote_openai_config_override_set(&self.backend_id, self.previous.clone());
    }
}

#[cfg(test)]
pub(crate) struct TestOpenAIConfigOverrideGuard {
    _inner: TestRemoteOpenAIConfigOverrideGuard,
}

#[cfg(test)]
impl TestOpenAIConfigOverrideGuard {
    pub(crate) fn set(config: crate::config::OpenAIResponsesConfig) -> Self {
        Self {
            _inner: TestRemoteOpenAIConfigOverrideGuard::set("openai-responses", config.into()),
        }
    }
}
