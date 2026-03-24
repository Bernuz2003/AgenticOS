use agentic_control_models::{BackendCapabilitiesView, BackendTelemetryView};
use anyhow::{Error as E, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokenizers::Tokenizer;

use crate::accounting::BackendAccountingEvent;
use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::model_catalog::ResolvedModelTarget;
use crate::prompting::{GenerationConfig, PromptFamily};

pub(crate) mod http;
mod local;
mod remote;
mod remote_adapter;

pub(crate) use local::diagnose_external_backend;
pub(crate) use local::managed_runtime_views;
pub(crate) use local::shutdown_managed_runtimes;
#[allow(unused_imports)]
pub(crate) use local::ExternalLlamaCppBackend;
use remote::RemoteOpenAICompatibleBackend;

#[cfg(test)]
pub(crate) use local::TestExternalEndpointOverrideGuard;
#[cfg(test)]
pub(crate) use local::TestExternalRuntimeReadyGuard;
#[cfg(test)]
pub(crate) use remote::{TestOpenAIConfigOverrideGuard, TestRemoteOpenAIConfigOverrideGuard};
#[cfg(test)]
use remote_adapter::{combine_completion_text, completion_is_finished, CompletionResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendClass {
    ResidentLocal,
    #[allow(dead_code)]
    RemoteStateless,
}

impl BackendClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResidentLocal => "resident_local",
            Self::RemoteStateless => "remote_stateless",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackendCapabilities {
    pub resident_kv: bool,
    pub persistent_slots: bool,
    pub save_restore_slots: bool,
    pub prompt_cache_reuse: bool,
    pub streaming_generation: bool,
    pub structured_output: bool,
    pub cancel_generation: bool,
    pub memory_telemetry: bool,
    pub tool_pause_resume: bool,
    pub context_compaction_reset: bool,
    pub parallel_sessions: bool,
}

pub fn runtime_backend_telemetry(backend_id: &str) -> Option<BackendTelemetryView> {
    remote::runtime_backend_telemetry(backend_id)
}

pub(crate) fn remote_runtime_config_for_backend(
    backend_id: &str,
) -> Option<crate::config::RemoteProviderRuntimeConfig> {
    remote::runtime_config(backend_id)
}

impl From<BackendCapabilities> for BackendCapabilitiesView {
    fn from(value: BackendCapabilities) -> Self {
        Self {
            resident_kv: value.resident_kv,
            persistent_slots: value.persistent_slots,
            save_restore_slots: value.save_restore_slots,
            prompt_cache_reuse: value.prompt_cache_reuse,
            streaming_generation: value.streaming_generation,
            structured_output: value.structured_output,
            cancel_generation: value.cancel_generation,
            memory_telemetry: value.memory_telemetry,
            tool_pause_resume: value.tool_pause_resume,
            context_compaction_reset: value.context_compaction_reset,
            parallel_sessions: value.parallel_sessions,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DriverDescriptor {
    pub id: &'static str,
    pub kind: &'static str,
    pub class: BackendClass,
    pub capabilities: BackendCapabilities,
    pub available: bool,
    pub load_supported: bool,
    pub note: &'static str,
    families: &'static [PromptFamily],
    architectures: &'static [&'static str],
}

impl DriverDescriptor {
    fn supports_family(&self, family: PromptFamily) -> bool {
        self.families.contains(&family)
    }

    fn supports_architecture(&self, architecture: Option<&str>) -> bool {
        architecture.is_none()
            || self.architectures.is_empty()
            || self.architectures.iter().any(|candidate| {
                architecture.is_some_and(|arch| candidate.eq_ignore_ascii_case(arch))
            })
    }

    fn supports_model(&self, family: PromptFamily, architecture: Option<&str>) -> bool {
        self.supports_family(family) && self.supports_architecture(architecture)
    }
}

#[derive(Debug, Clone)]
pub struct DriverResolution {
    pub resolved_backend_id: String,
    pub backend_class: BackendClass,
    pub capabilities: BackendCapabilities,
    pub resolution_source: &'static str,
    pub resolution_rationale: String,
    pub available: bool,
    pub load_supported: bool,
}

pub struct InferenceStepRequest<'a> {
    pub context_slot_id: Option<ContextSlotId>,
    pub tokens: &'a [u32],
    pub rendered_prompt: &'a str,
    pub resident_prompt_suffix: &'a str,
    pub index_pos: usize,
    pub remaining_generation_budget: usize,
    pub tokenizer: &'a Tokenizer,
    pub generation: GenerationConfig,
    pub stream_observer: Option<&'a mut dyn StreamChunkObserver>,
    #[allow(dead_code)]
    pub eos_token_id: u32,
    #[allow(dead_code)]
    pub eot_token_id: u32,
}

pub trait StreamChunkObserver {
    fn on_chunk(&mut self, chunk: &str);
}

impl<F> StreamChunkObserver for F
where
    F: FnMut(&str),
{
    fn on_chunk(&mut self, chunk: &str) {
        self(chunk);
    }
}

const DRIVER_REGISTRY: [DriverDescriptor; 4] = [
    local::EXTERNAL_LLAMACPP_DRIVER,
    remote::OPENAI_RESPONSES_DRIVER,
    remote::GROQ_RESPONSES_DRIVER,
    remote::OPENROUTER_DRIVER,
];

pub fn driver_registry() -> &'static [DriverDescriptor] {
    &DRIVER_REGISTRY
}

pub fn driver_descriptor(backend_id: &str) -> Option<&'static DriverDescriptor> {
    DRIVER_REGISTRY
        .iter()
        .find(|driver| driver.id == backend_id)
}

pub(crate) fn ensure_runtime_backend_ready_for_target(
    backend_id: &str,
    family: PromptFamily,
    _reference: &str,
) -> Result<(), String> {
    match backend_id {
        "external-llamacpp" => local::runtime_manager::ensure_runtime_ready_for_family(family)
            .map_err(|err| {
                format!(
                    "external-llamacpp is unavailable for family '{:?}': {}",
                    family, err
                )
            }),
        _ => Ok(()),
    }
}

fn is_driver_runtime_loadable(driver: &DriverDescriptor) -> bool {
    match driver.id {
        "external-llamacpp" => local::runtime_manager::runtime_driver_available(),
        "openai-responses" | "groq-responses" | "openrouter" => remote::runtime_ready(driver.id),
        _ => driver.available && driver.load_supported,
    }
}

fn runtime_driver_flags(driver: &DriverDescriptor) -> (bool, bool) {
    match driver.id {
        "external-llamacpp" => {
            let ready = local::runtime_manager::runtime_driver_available();
            (ready, ready)
        }
        "openai-responses" | "groq-responses" | "openrouter" => {
            let ready = remote::runtime_ready(driver.id);
            (ready, ready)
        }
        _ => (driver.available, driver.load_supported),
    }
}

fn driver_loadability_detail(driver: &DriverDescriptor) -> String {
    match driver.id {
        "external-llamacpp" => local::runtime_driver_unavailability_reason()
            .unwrap_or_else(|| driver.note.to_string()),
        _ => driver.note.to_string(),
    }
}

fn default_runtime_driver_for_model(
    family: PromptFamily,
    architecture: Option<&str>,
) -> Option<&'static DriverDescriptor> {
    if let Some(driver) = DRIVER_REGISTRY.iter().find(|driver| {
        driver.id == local::DEFAULT_RESIDENT_BACKEND_ID
            && driver.supports_model(family, architecture)
            && is_driver_runtime_loadable(driver)
    }) {
        return Some(driver);
    }

    DRIVER_REGISTRY.iter().find(|driver| {
        driver.class == BackendClass::ResidentLocal
            && driver.id != local::DEFAULT_RESIDENT_BACKEND_ID
            && driver.supports_model(family, architecture)
            && is_driver_runtime_loadable(driver)
    })
}

pub(crate) fn persist_context_slot_payload_for_backend(
    backend_id: &str,
    family: PromptFamily,
    slot_id: ContextSlotId,
    final_path: &Path,
) -> Result<SlotPersistenceKind, String> {
    local::persist_context_slot_payload_for_backend(backend_id, family, slot_id, final_path)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_driver_for_family(
    family: PromptFamily,
    backend_preference: Option<&str>,
) -> std::result::Result<DriverResolution, String> {
    resolve_driver_for_model(family, None, backend_preference)
}

pub fn resolve_driver_for_model(
    family: PromptFamily,
    architecture: Option<&str>,
    backend_preference: Option<&str>,
) -> std::result::Result<DriverResolution, String> {
    if matches!(family, PromptFamily::Unknown) && backend_preference.is_none() {
        return Err(
            "Cannot resolve driver for unknown model family without an explicit backend."
                .to_string(),
        );
    }

    let fallback = default_runtime_driver_for_model(family, architecture);

    let architecture_label = architecture
        .map(|value| format!(" architecture '{}'", value))
        .unwrap_or_default();

    if let Some(preferred_id) = backend_preference {
        if let Some(driver) = DRIVER_REGISTRY.iter().find(|item| item.id == preferred_id) {
            if !driver.supports_family(family) {
                return Err(format!(
                    "Preferred driver '{}' does not support family {:?}.",
                    preferred_id, family
                ));
            }

            if !driver.supports_architecture(architecture) {
                return Err(format!(
                    "Preferred driver '{}' does not support{:?} for family {:?}.",
                    preferred_id, architecture, family
                ));
            }

            if is_driver_runtime_loadable(driver) {
                let (available, load_supported) = runtime_driver_flags(driver);
                return Ok(DriverResolution {
                    resolved_backend_id: driver.id.to_string(),
                    backend_class: driver.class,
                    capabilities: driver.capabilities,
                    resolution_source: "metadata-preference",
                    resolution_rationale: format!(
                        "using preferred driver '{}' declared by model metadata for family {:?}{}",
                        preferred_id, family, architecture_label
                    ),
                    available,
                    load_supported,
                });
            }

            if let Some(fallback_driver) = fallback {
                let (available, load_supported) = runtime_driver_flags(fallback_driver);
                return Ok(DriverResolution {
                    resolved_backend_id: fallback_driver.id.to_string(),
                    backend_class: fallback_driver.class,
                    capabilities: fallback_driver.capabilities,
                    resolution_source: "metadata-preference-fallback",
                    resolution_rationale: format!(
                        "preferred driver '{}' is registered but not loadable yet for family {:?}{}; falling back to '{}': {}",
                        preferred_id,
                        family,
                        architecture_label,
                        fallback_driver.id,
                        driver_loadability_detail(driver)
                    ),
                    available,
                    load_supported,
                });
            }

            return Err(format!(
                "Preferred driver '{}' is registered but not loadable, and no compatible fallback is available for family {:?}{}: {}",
                preferred_id,
                family,
                architecture_label,
                driver_loadability_detail(driver)
            ));
        }

        if let Some(fallback_driver) = fallback {
            let (available, load_supported) = runtime_driver_flags(fallback_driver);
            return Ok(DriverResolution {
                resolved_backend_id: fallback_driver.id.to_string(),
                backend_class: fallback_driver.class,
                capabilities: fallback_driver.capabilities,
                resolution_source: "metadata-preference-unknown-fallback",
                resolution_rationale: format!(
                    "preferred driver '{}' is unknown; falling back to '{}' for family {:?}{}",
                    preferred_id, fallback_driver.id, family, architecture_label
                ),
                available,
                load_supported,
            });
        }

        return Err(format!(
            "Preferred driver '{}' is unknown and no compatible fallback is available for family {:?}{}.",
            preferred_id, family, architecture_label
        ));
    }

    if let Some(driver) = fallback {
        let (available, load_supported) = runtime_driver_flags(driver);
        return Ok(DriverResolution {
            resolved_backend_id: driver.id.to_string(),
            backend_class: driver.class,
            capabilities: driver.capabilities,
            resolution_source: "family-default",
            resolution_rationale: format!(
                "using default loadable driver '{}' for family {:?}{}",
                driver.id, family, architecture_label
            ),
            available,
            load_supported,
        });
    }

    if let Some(driver) = DRIVER_REGISTRY.iter().find(|driver| {
        driver.supports_model(family, architecture) && !is_driver_runtime_loadable(driver)
    }) {
        return Err(format!(
            "No registered loadable driver can satisfy family {:?}{}: {}",
            family,
            architecture_label,
            driver_loadability_detail(driver)
        ));
    }

    Err(format!(
        "No registered loadable driver can satisfy family {:?}{}.",
        family, architecture_label
    ))
}

pub trait InferenceBackend: Send {
    fn backend_id(&self) -> &'static str;
    fn family(&self) -> PromptFamily;
    fn generate_step(&mut self, request: InferenceStepRequest<'_>) -> Result<InferenceStepResult>;
    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>>;
    fn take_last_accounting_event(&mut self) -> Option<BackendAccountingEvent> {
        None
    }
    fn runtime_capabilities(&self) -> Option<BackendCapabilities> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct InferenceStepResult {
    pub appended_tokens: Vec<u32>,
    pub emitted_text: String,
    pub finished: bool,
    pub finish_reason: Option<InferenceFinishReason>,
    pub next_index_pos: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceFinishReason {
    ModelStop,
    TurnBudgetExhausted,
}

impl InferenceFinishReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelStop => "model_stop",
            Self::TurnBudgetExhausted => "turn_budget_exhausted",
        }
    }
}

#[allow(dead_code)]
pub trait ContextSlotPersistence: InferenceBackend {
    fn save_context_slot(&self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        Err(E::msg(format!(
            "Backend '{}' does not yet support saving context slot {} to {}.",
            InferenceBackend::backend_id(self),
            slot_id,
            path.display()
        )))
    }

    fn load_context_slot(&mut self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        Err(E::msg(format!(
            "Backend '{}' does not yet support loading context slot {} from {}.",
            InferenceBackend::backend_id(self),
            slot_id,
            path.display()
        )))
    }

    fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
        Err(E::msg(format!(
            "Backend '{}' does not yet support freeing context slot {}.",
            InferenceBackend::backend_id(self),
            slot_id
        )))
    }
}

pub trait ModelBackend: InferenceBackend + ContextSlotPersistence {}

impl<T> ModelBackend for T where T: InferenceBackend + ContextSlotPersistence {}

pub struct RuntimeModel {
    inner: Box<dyn ModelBackend>,
}

impl RuntimeModel {
    #[cfg(test)]
    pub(crate) fn from_boxed_backend(inner: Box<dyn ModelBackend>) -> Self {
        Self { inner }
    }

    #[allow(dead_code)]
    pub fn load_from_gguf(_path: &str, family: PromptFamily, backend_id: &str) -> Result<Self> {
        Self::load_from_reference(_path, family, backend_id)
    }

    pub fn load_target(target: &ResolvedModelTarget) -> Result<Self> {
        let descriptor = resolve_loadable_driver_descriptor(
            &target.driver_resolution().resolved_backend_id,
            target.family(),
        )?;

        let backend: Box<dyn ModelBackend> = match target {
            ResolvedModelTarget::Local(local) => match descriptor.id {
                "external-llamacpp" => Box::new(local::backend_for_target(local).map_err(E::msg)?),
                _ => {
                    return Err(E::msg(format!(
                        "Backend '{}' is registered but has no typed local loader implementation.",
                        descriptor.id
                    )))
                }
            },
            ResolvedModelTarget::Remote(remote) => match (&remote.runtime_config, descriptor.id) {
                (config, "openai-responses")
                | (config, "groq-responses")
                | (config, "openrouter") => Box::new(RemoteOpenAICompatibleBackend::from_runtime(
                    remote.family,
                    descriptor.id,
                    remote.model_spec.clone(),
                    config.clone(),
                )?),
                _ => {
                    return Err(E::msg(format!(
                        "Backend '{}' is registered but has no typed remote loader implementation.",
                        descriptor.id
                    )))
                }
            },
        };

        Ok(Self { inner: backend })
    }

    pub fn load_from_reference(
        reference: &str,
        family: PromptFamily,
        backend_id: &str,
    ) -> Result<Self> {
        let descriptor = resolve_loadable_driver_descriptor(backend_id, family)?;

        let backend: Box<dyn ModelBackend> = match descriptor.id {
            "external-llamacpp" => Box::new(
                local::backend_for_reference(reference, family).map_err(E::msg)?,
            ),
            "openai-responses" | "groq-responses" | "openrouter" => Box::new(
                RemoteOpenAICompatibleBackend::from_env(family, descriptor.id, reference)?,
            ),
            _ => {
                return Err(E::msg(format!(
                    "Backend '{}' is registered but has no in-process loader implementation.",
                    descriptor.id
                )))
            }
        };

        Ok(Self { inner: backend })
    }

    pub fn backend_id(&self) -> &'static str {
        self.inner.backend_id()
    }

    pub fn family(&self) -> PromptFamily {
        self.inner.family()
    }

    pub fn backend_class(&self) -> BackendClass {
        driver_descriptor(self.backend_id())
            .map(|driver| driver.class)
            .unwrap_or(BackendClass::RemoteStateless)
    }

    pub fn backend_capabilities(&self) -> BackendCapabilities {
        self.inner.runtime_capabilities().unwrap_or_else(|| {
            driver_descriptor(self.backend_id())
                .map(|driver| driver.capabilities)
                .unwrap_or_default()
        })
    }

    pub fn backend_telemetry(&self) -> Option<BackendTelemetryView> {
        runtime_backend_telemetry(self.backend_id())
    }

    pub fn generate_step(
        &mut self,
        request: InferenceStepRequest<'_>,
    ) -> Result<InferenceStepResult> {
        self.inner.generate_step(request)
    }

    pub fn take_last_accounting_event(&mut self) -> Option<BackendAccountingEvent> {
        self.inner.take_last_accounting_event()
    }

    #[allow(dead_code)]
    pub fn save_context_slot(&self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        self.inner.save_context_slot(slot_id, path)
    }

    #[allow(dead_code)]
    pub fn load_context_slot(&mut self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        self.inner.load_context_slot(slot_id, path)
    }

    #[allow(dead_code)]
    pub fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
        self.inner.free_context_slot(slot_id)
    }

    /// Clone the model weights for a new process (zero-copy for backends that support it).
    ///
    /// Returns `None` for non-cloneable backends. The caller must enforce any
    /// single-process guard required by the selected backend.
    pub fn duplicate_if_supported(&self) -> Option<Self> {
        self.inner.duplicate_boxed().map(|inner| Self { inner })
    }
}

fn resolve_loadable_driver_descriptor(
    backend_id: &str,
    family: PromptFamily,
) -> Result<&'static DriverDescriptor> {
    let descriptor = driver_registry()
        .iter()
        .find(|driver| driver.id == backend_id)
        .ok_or_else(|| E::msg(format!("Unknown backend id '{}'.", backend_id)))?;

    if !descriptor.supports_family(family) {
        return Err(E::msg(format!(
            "Backend '{}' does not support family {:?}.",
            backend_id, family
        )));
    }

    if !is_driver_runtime_loadable(descriptor) {
        return Err(E::msg(format!(
            "Backend '{}' is registered as '{}' but is not loadable yet: {}",
            backend_id, descriptor.kind, descriptor.note
        )));
    }

    Ok(descriptor)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
