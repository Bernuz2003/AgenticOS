use agentic_control_models::BackendCapabilitiesView;
use anyhow::{Error as E, Result};
use std::path::Path;
use tokenizers::Tokenizer;

use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::prompting::{GenerationConfig, PromptFamily};

mod diagnostics;
mod external_llamacpp;
pub(crate) mod http;
mod remote_adapter;

pub(crate) use diagnostics::diagnose_external_backend;
use external_llamacpp::ExternalLlamaCppBackend;

#[cfg(test)]
use remote_adapter::{combine_completion_text, completion_is_finished, CompletionResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    #[allow(dead_code)]
    pub eos_token_id: u32,
    #[allow(dead_code)]
    pub eot_token_id: u32,
}

const FAMILIES_COMMON: [PromptFamily; 3] = [
    PromptFamily::Llama,
    PromptFamily::Qwen,
    PromptFamily::Mistral,
];
const ARCH_ANY: [&str; 0] = [];
const CAP_EXTERNAL_LLAMACPP: BackendCapabilities = BackendCapabilities {
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
const DEFAULT_RESIDENT_BACKEND_ID: &str = "external-llamacpp";

const DRIVER_REGISTRY: [DriverDescriptor; 1] = [DriverDescriptor {
    id: "external-llamacpp",
    kind: "resident-adapter",
    class: BackendClass::ResidentLocal,
    capabilities: CAP_EXTERNAL_LLAMACPP,
    available: false,
    load_supported: false,
    note: "Resident local llama.cpp adapter exposed through llama-server.",
    families: &FAMILIES_COMMON,
    architectures: &ARCH_ANY,
}];

pub fn driver_registry() -> &'static [DriverDescriptor] {
    &DRIVER_REGISTRY
}

pub fn driver_descriptor(backend_id: &str) -> Option<&'static DriverDescriptor> {
    DRIVER_REGISTRY
        .iter()
        .find(|driver| driver.id == backend_id)
}

fn external_llamacpp_endpoint() -> Option<String> {
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

#[cfg(test)]
pub(crate) fn test_external_endpoint_override_get() -> Option<String> {
    let cell = test_external_endpoint_override_cell();
    cell.lock()
        .expect("lock external endpoint override")
        .clone()
}

#[cfg(test)]
pub(crate) fn test_external_endpoint_override_set(value: Option<String>) {
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

fn is_driver_runtime_loadable(driver: &DriverDescriptor) -> bool {
    match driver.id {
        "external-llamacpp" => external_llamacpp_endpoint().is_some(),
        _ => driver.available && driver.load_supported,
    }
}

fn runtime_driver_flags(driver: &DriverDescriptor) -> (bool, bool) {
    match driver.id {
        "external-llamacpp" => {
            let ready = external_llamacpp_endpoint().is_some();
            (ready, ready)
        }
        _ => (driver.available, driver.load_supported),
    }
}

fn default_runtime_driver_for_model(
    family: PromptFamily,
    architecture: Option<&str>,
) -> Option<&'static DriverDescriptor> {
    if let Some(driver) = DRIVER_REGISTRY.iter().find(|driver| {
        driver.id == DEFAULT_RESIDENT_BACKEND_ID
            && driver.supports_model(family, architecture)
            && is_driver_runtime_loadable(driver)
    }) {
        return Some(driver);
    }

    DRIVER_REGISTRY.iter().find(|driver| {
        driver.id != DEFAULT_RESIDENT_BACKEND_ID
            && driver.supports_model(family, architecture)
            && is_driver_runtime_loadable(driver)
    })
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
    if matches!(family, PromptFamily::Unknown) {
        return Err("Cannot resolve driver for unknown model family.".to_string());
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
                        preferred_id, family, architecture_label, fallback_driver.id, driver.note
                    ),
                    available,
                    load_supported,
                });
            }

            return Err(format!(
                "Preferred driver '{}' is registered but not loadable, and no compatible fallback is available for family {:?}{}.",
                preferred_id, family, architecture_label
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

    pub fn load_from_gguf(_path: &str, family: PromptFamily, backend_id: &str) -> Result<Self> {
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

        let backend: Box<dyn ModelBackend> = match backend_id {
            "external-llamacpp" => Box::new(ExternalLlamaCppBackend::from_env(family)?),
            _ => {
                return Err(E::msg(format!(
                    "Backend '{}' is registered but has no in-process loader implementation.",
                    backend_id
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
        driver_descriptor(self.backend_id())
            .map(|driver| driver.capabilities)
            .unwrap_or_default()
    }

    pub fn generate_step(
        &mut self,
        request: InferenceStepRequest<'_>,
    ) -> Result<InferenceStepResult> {
        self.inner.generate_step(request)
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

#[cfg(test)]
mod tests {
    use super::{
        combine_completion_text, completion_is_finished, diagnose_external_backend,
        persist_context_slot_payload_for_backend, resolve_driver_for_family,
        resolve_driver_for_model, BackendClass, CompletionResponse, ContextSlotPersistence,
        ExternalLlamaCppBackend, InferenceBackend, InferenceStepRequest, InferenceStepResult,
        PromptFamily, RuntimeModel, TestExternalEndpointOverrideGuard,
    };
    use crate::memory::{ContextSlotId, SlotPersistenceKind};
    use crate::prompting::GenerationConfig;
    use anyhow::Result;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::Tokenizer;

    type SharedStringLog = Arc<Mutex<Vec<String>>>;
    type MockServerHandle = thread::JoinHandle<()>;

    fn test_tokenizer() -> Tokenizer {
        let vocab = [("<unk>".to_string(), 0), ("hello".to_string(), 1)]
            .into_iter()
            .collect();

        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build wordlevel tokenizer");

        Tokenizer::new(model)
    }

    fn spawn_mock_llamacpp_server(
        expected_requests: usize,
    ) -> (String, SharedStringLog, SharedStringLog, MockServerHandle) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let address = listener.local_addr().expect("mock server addr");
        let paths = Arc::new(Mutex::new(Vec::new()));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let paths_for_thread = Arc::clone(&paths);
        let bodies_for_thread = Arc::clone(&bodies);

        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("accept mock request");
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("read mock request");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let body = request
                    .split("\r\n\r\n")
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                paths_for_thread
                    .lock()
                    .expect("lock paths")
                    .push(path.clone());
                bodies_for_thread.lock().expect("lock bodies").push(body);

                let body = match path.as_str() {
                    "/completion" => r#"{"content":"hello","tokens":[1]}"#,
                    "/slots/7?action=save"
                    | "/slots/7?action=restore"
                    | "/slots/7?action=erase" => r#"{"ok":true}"#,
                    _ => r#"{"error":"unexpected path"}"#,
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock response");
            }
        });

        (format!("http://{}", address), paths, bodies, handle)
    }

    fn spawn_mock_streaming_tool_server(
    ) -> (String, SharedStringLog, SharedStringLog, MockServerHandle) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock streaming server");
        let address = listener.local_addr().expect("mock streaming addr");
        let paths = Arc::new(Mutex::new(Vec::new()));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let paths_for_thread = Arc::clone(&paths);
        let bodies_for_thread = Arc::clone(&bodies);

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock streaming request");
            let mut buffer = [0_u8; 4096];
            let read = stream
                .read(&mut buffer)
                .expect("read mock streaming request");
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            let body = request
                .split("\r\n\r\n")
                .nth(1)
                .unwrap_or_default()
                .to_string();
            paths_for_thread
                .lock()
                .expect("lock streaming paths")
                .push(path);
            bodies_for_thread
                .lock()
                .expect("lock streaming bodies")
                .push(body);

            let chunk_one =
                "data: {\"content\":\"TOOL:calc {\\\"expression\\\":\\\"1+1\\\"}\",\"stop\":false}\n\n";
            let chunk_two = "data: {\"content\":\"\\nignored tail\",\"stop\":false}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                chunk_one.len(),
                chunk_one,
                chunk_two.len(),
                chunk_two,
            );
            let _ = stream.write_all(response.as_bytes());
        });

        (format!("http://{}", address), paths, bodies, handle)
    }

    fn spawn_mock_diag_server() -> (String, SharedStringLog, MockServerHandle) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock diag server");
        let address = listener.local_addr().expect("mock diag server addr");
        let paths = Arc::new(Mutex::new(Vec::new()));
        let paths_for_thread = Arc::clone(&paths);

        let handle = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept mock diag request");
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("read mock diag request");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                paths_for_thread
                    .lock()
                    .expect("lock diag paths")
                    .push(path.clone());

                let (status, body) = match path.as_str() {
                    "/health" => ("HTTP/1.1 200 OK", r#"{"status":"ok"}"#),
                    "/props" => (
                        "HTTP/1.1 200 OK",
                        r#"{"model_path":"/models/qwen3.5.gguf","total_slots":4}"#,
                    ),
                    "/slots" => ("HTTP/1.1 200 OK", r#"[{"id":0},{"id":1}]"#),
                    _ => ("HTTP/1.1 404 Not Found", r#"{"error":"unexpected path"}"#),
                };
                let response = format!(
                    "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock diag response");
            }
        });

        (format!("http://{}", address), paths, handle)
    }

    #[test]
    fn family_default_requires_loadable_external_runtime() {
        let _endpoint = TestExternalEndpointOverrideGuard::clear();
        let err = resolve_driver_for_family(PromptFamily::Llama, None)
            .expect_err("llama driver should require a configured resident runtime");
        assert!(err.contains("No registered loadable driver"));
    }

    #[test]
    fn family_default_prefers_external_runtime_when_configured() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");

        let resolution =
            resolve_driver_for_family(PromptFamily::Llama, None).expect("resolve llama driver");

        assert_eq!(resolution.resolved_backend_id, "external-llamacpp");
        assert_eq!(resolution.backend_class, BackendClass::ResidentLocal);
        assert_eq!(resolution.resolution_source, "family-default");
        assert!(resolution.capabilities.persistent_slots);
    }

    #[test]
    fn preferred_external_driver_errors_when_endpoint_is_missing() {
        let _endpoint = TestExternalEndpointOverrideGuard::clear();
        let err = resolve_driver_for_family(PromptFamily::Qwen, Some("external-llamacpp"))
            .expect_err("external backend should fail when endpoint is missing");
        assert!(err.contains("not loadable"));
    }

    #[test]
    fn unsupported_family_without_loadable_driver_errors() {
        let _endpoint = TestExternalEndpointOverrideGuard::clear();
        let err = resolve_driver_for_family(PromptFamily::Mistral, None)
            .expect_err("mistral should not resolve to loadable driver yet");
        assert!(err.contains("No registered loadable driver"));
    }

    #[test]
    fn architecture_specific_driver_resolution_rejects_qwen35_for_qwen2_backend() {
        let err = resolve_driver_for_model(PromptFamily::Qwen, Some("qwen35"), None)
            .expect_err("qwen35 should not resolve to qwen2 backend");
        assert!(err.contains("qwen35"));
    }

    #[test]
    fn architecture_specific_driver_resolution_uses_external_rpc_when_configured() {
        let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");

        let resolution = resolve_driver_for_model(PromptFamily::Qwen, Some("qwen35"), None)
            .expect("qwen35 should resolve to external rpc when configured");

        assert_eq!(resolution.resolved_backend_id, "external-llamacpp");
        assert_eq!(resolution.backend_class, BackendClass::ResidentLocal);
        assert_eq!(resolution.resolution_source, "family-default");
        assert!(resolution.available);
        assert!(resolution.load_supported);
        assert!(resolution.capabilities.persistent_slots);
    }

    struct DummyBackend;

    impl InferenceBackend for DummyBackend {
        fn backend_id(&self) -> &'static str {
            "dummy"
        }

        fn family(&self) -> PromptFamily {
            PromptFamily::Unknown
        }

        fn generate_step(
            &mut self,
            _request: InferenceStepRequest<'_>,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in this test");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn super::ModelBackend>> {
            None
        }
    }

    impl ContextSlotPersistence for DummyBackend {}

    #[test]
    fn runtime_model_exposes_context_slot_boundary_with_unsupported_default() {
        let mut model = RuntimeModel::from_boxed_backend(Box::new(DummyBackend));

        let save_err = model
            .save_context_slot(
                ContextSlotId::from(7_u64),
                Path::new("workspace/swap/pid_7.swap"),
            )
            .expect_err("default context slot persistence should be unsupported");
        let load_err = model
            .load_context_slot(
                ContextSlotId::from(7_u64),
                Path::new("workspace/swap/pid_7.swap"),
            )
            .expect_err("default context slot load should be unsupported");
        let free_err = model
            .free_context_slot(ContextSlotId::from(7_u64))
            .expect_err("default context slot free should be unsupported");

        assert!(save_err
            .to_string()
            .contains("does not yet support saving context slot 7"));
        assert!(load_err
            .to_string()
            .contains("does not yet support loading context slot 7"));
        assert!(free_err
            .to_string()
            .contains("does not yet support freeing context slot 7"));
    }

    struct RecordingBackend {
        freed_slots: Arc<Mutex<Vec<ContextSlotId>>>,
    }

    impl InferenceBackend for RecordingBackend {
        fn backend_id(&self) -> &'static str {
            "recording"
        }

        fn family(&self) -> PromptFamily {
            PromptFamily::Llama
        }

        fn generate_step(
            &mut self,
            _request: InferenceStepRequest<'_>,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in this test");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn super::ModelBackend>> {
            None
        }
    }

    impl ContextSlotPersistence for RecordingBackend {
        fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
            self.freed_slots
                .lock()
                .expect("lock freed slots")
                .push(slot_id);
            Ok(())
        }
    }

    #[test]
    fn runtime_model_can_delegate_backend_slot_free() {
        let freed_slots = Arc::new(Mutex::new(Vec::new()));
        let backend = RecordingBackend {
            freed_slots: Arc::clone(&freed_slots),
        };
        let mut model = RuntimeModel::from_boxed_backend(Box::new(backend));

        model
            .free_context_slot(11)
            .expect("backend-specific free_context_slot should succeed");

        assert_eq!(
            freed_slots.lock().expect("lock freed slots").as_slice(),
            &[11]
        );
    }

    #[test]
    fn external_backend_roundtrips_generation_and_slot_rpc() {
        let (endpoint, paths, bodies, server_handle) = spawn_mock_llamacpp_server(4);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let mut model =
            RuntimeModel::load_from_gguf("ignored.gguf", PromptFamily::Qwen, "external-llamacpp")
                .expect("load external runtime model");
        assert_eq!(model.backend_class(), BackendClass::ResidentLocal);
        assert!(model.backend_capabilities().save_restore_slots);
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 16,
        };

        let step = model
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(7),
                tokens: &[1],
                rendered_prompt: "hello",
                resident_prompt_suffix: "hello",
                index_pos: 0,
                remaining_generation_budget: generation.max_tokens,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 2,
                eot_token_id: 3,
            })
            .expect("generate step through external backend");

        assert_eq!(step.emitted_text, "hello");
        assert_eq!(step.appended_tokens, vec![1]);
        assert_eq!(step.next_index_pos, 1);

        let swap_path = Path::new("workspace/swap/pid_7.slot");
        model
            .save_context_slot(7, swap_path)
            .expect("save context slot via rpc");
        model
            .load_context_slot(7, swap_path)
            .expect("load context slot via rpc");
        model
            .free_context_slot(7)
            .expect("free context slot via rpc");

        server_handle.join().expect("join mock server");

        assert_eq!(
            paths.lock().expect("lock paths").as_slice(),
            &[
                "/completion",
                "/slots/7?action=save",
                "/slots/7?action=restore",
                "/slots/7?action=erase",
            ]
        );

        let request_bodies = bodies.lock().expect("lock bodies");
        assert!(
            request_bodies
                .first()
                .map(|body| body.contains("\"prompt\":\"hello\""))
                .unwrap_or(false),
            "completion request should include the prompt body"
        );
    }

    #[test]
    fn external_backend_streaming_stops_on_tool_marker() {
        let (endpoint, paths, bodies, server_handle) = spawn_mock_streaming_tool_server();
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let mut model =
            RuntimeModel::load_from_gguf("ignored.gguf", PromptFamily::Qwen, "external-llamacpp")
                .expect("load external runtime model");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 32,
        };

        let step = model
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(7),
                tokens: &[1],
                rendered_prompt: "hello",
                resident_prompt_suffix: "hello",
                index_pos: 0,
                remaining_generation_budget: generation.max_tokens,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 2,
                eot_token_id: 3,
            })
            .expect("streaming generate step through external backend");

        server_handle.join().expect("join streaming mock server");

        assert_eq!(
            paths.lock().expect("lock paths").as_slice(),
            &["/completion"]
        );
        assert_eq!(step.emitted_text, "TOOL:calc {\"expression\":\"1+1\"}");
        assert_eq!(
            step.appended_tokens,
            tokenizer
                .encode(step.emitted_text.as_str(), false)
                .expect("encode emitted tool invocation")
                .get_ids()
                .to_vec()
        );
        assert!(!step.finished);

        assert!(
            bodies
                .lock()
                .expect("lock bodies")
                .first()
                .map(|body| body.contains("\"stream\":true"))
                .unwrap_or(false),
            "streaming completion request should enable llama.cpp streaming mode"
        );
    }

    #[test]
    fn persist_context_slot_payload_uses_external_slot_save_for_resident_backend() {
        let (endpoint, paths, _bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);
        let base = Path::new("workspace/test_external_slot_persist");
        let _ = fs::remove_dir_all(base);
        fs::create_dir_all(base).expect("create external persist test dir");
        let final_path = base.join("pid_1_slot_7.swap");

        let persistence_kind =
            persist_context_slot_payload_for_backend("external-llamacpp", 7, &final_path)
                .expect("resident backend slot persist should succeed");

        server_handle.join().expect("join mock server");

        assert_eq!(persistence_kind, SlotPersistenceKind::BackendSlotSnapshot);
        assert!(
            !final_path.exists(),
            "external slot save should not create a local payload file"
        );
        assert_eq!(
            paths.lock().expect("lock paths").as_slice(),
            &["/slots/7?action=save"]
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn external_backend_preserves_special_tokens_in_prompt_decode() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let vocab = [
            ("<unk>".to_string(), 0),
            ("<|im_start|>".to_string(), 1),
            ("user".to_string(), 2),
            ("hi".to_string(), 3),
            ("<|im_end|>".to_string(), 4),
            ("<|im_start|>assistant".to_string(), 5),
        ]
        .into_iter()
        .collect();

        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build wordlevel tokenizer");
        let tokenizer = Tokenizer::new(model);

        let mut backend = ExternalLlamaCppBackend::from_env(PromptFamily::Qwen)
            .expect("build external backend from endpoint override");
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 16,
        };

        backend
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(3),
                tokens: &[1, 2, 3, 4, 5],
                rendered_prompt: "<|im_start|>userhi<|im_end|><|im_start|>assistant",
                resident_prompt_suffix: "<|im_start|>userhi<|im_end|><|im_start|>assistant",
                index_pos: 0,
                remaining_generation_budget: generation.max_tokens,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 6,
                eot_token_id: 7,
            })
            .expect("generate step should succeed");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            body.contains("<|im_start|>"),
            "special chat tokens must survive prompt decode"
        );
        assert!(
            body.contains("<|im_end|>"),
            "end markers must survive prompt decode"
        );
    }

    #[test]
    fn external_backend_uses_chunked_completion_requests() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);
        let expected_chunk_tokens = crate::config::kernel_config()
            .external_llamacpp
            .chunk_tokens;

        let mut backend = ExternalLlamaCppBackend::from_env(PromptFamily::Qwen)
            .expect("build external backend from endpoint override");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 64,
        };

        backend
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(3),
                tokens: &[1],
                rendered_prompt: "hello",
                resident_prompt_suffix: "hello",
                index_pos: 0,
                remaining_generation_budget: generation.max_tokens,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 6,
                eot_token_id: 7,
            })
            .expect("generate step should succeed");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            body.contains(&format!("\"n_predict\":{expected_chunk_tokens}")),
            "external backend should request the configured completion chunk size"
        );
    }

    #[test]
    fn external_backend_uses_rendered_prompt_cache_instead_of_redecoding_tokens() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let mut backend = ExternalLlamaCppBackend::from_env(PromptFamily::Qwen)
            .expect("build external backend from endpoint override");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 16,
        };

        backend
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(3),
                tokens: &[1],
                rendered_prompt: "hello\nTOOL:calc {\"expression\":\"1+1\"}\nOutput:\n2\n",
                resident_prompt_suffix: "Output:\n2\n",
                index_pos: 0,
                remaining_generation_budget: generation.max_tokens,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 6,
                eot_token_id: 7,
            })
            .expect("generate step should succeed");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            body.contains("\\nTOOL:calc"),
            "external backend should honor the rendered prompt cache instead of re-decoding token ids"
        );
        assert!(
            body.contains("\"prompt\":\"hello\\nTOOL:calc {\\\"expression\\\":\\\"1+1\\\"}\\nOutput:\\n2\\n\""),
            "llama.cpp should still receive the full prompt when append-only transport is unavailable"
        );
    }

    #[test]
    fn external_backend_uses_remaining_turn_budget_not_total_context_len() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let mut backend = ExternalLlamaCppBackend::from_env(PromptFamily::Qwen)
            .expect("build external backend from endpoint override");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 16,
        };
        let long_context = vec![1_u32; 32];

        let step = backend
            .generate_step(InferenceStepRequest {
                context_slot_id: Some(3),
                tokens: &long_context,
                rendered_prompt: "hello",
                resident_prompt_suffix: "",
                index_pos: long_context.len(),
                remaining_generation_budget: 1,
                tokenizer: &tokenizer,
                generation,
                eos_token_id: 6,
                eot_token_id: 7,
            })
            .expect("generate step should still perform one more token of work");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(
            body.contains("\"n_predict\":1"),
            "external backend must honor the remaining turn budget"
        );
        assert_eq!(step.appended_tokens, vec![1]);
        assert!(
            step.finished,
            "one-token remaining budget should end the turn"
        );
    }

    #[test]
    fn external_backend_does_not_finish_on_stop_type_limit() {
        let response: CompletionResponse = serde_json::from_str(
            r#"{"content":"<think>\nThinking\n</think>","tokens":[1],"stop":true,"stop_type":"limit","truncated":false}"#,
        )
        .expect("deserialize completion response");

        assert!(!completion_is_finished(&response, None));
    }

    #[test]
    fn external_backend_combines_separate_reasoning_content() {
        let response: CompletionResponse = serde_json::from_str(
            r#"{"content":"4","reasoning_content":"Step 1: add 2 and 2.","tokens":[1],"stop":false,"stop_type":"none"}"#,
        )
        .expect("deserialize completion response");

        let emitted_text = combine_completion_text(
            response.reasoning_content.as_deref(),
            response.content.as_deref(),
            None,
            None,
        );

        assert_eq!(emitted_text, "<think>\nStep 1: add 2 and 2.\n</think>\n4");
    }

    #[test]
    fn external_backend_diagnostic_reports_health_props_and_slots() {
        let (endpoint, paths, server_handle) = spawn_mock_diag_server();
        let _endpoint = TestExternalEndpointOverrideGuard::set(&endpoint);

        let report = diagnose_external_backend().expect("diagnostic report should succeed");

        assert_eq!(report["backend"].as_str(), Some("external-llamacpp"));
        assert_eq!(report["backend_class"].as_str(), Some("resident_local"));
        assert_eq!(
            report["backend_capabilities"]["persistent_slots"].as_bool(),
            Some(true)
        );
        assert_eq!(report["health"]["status_code"].as_u64(), Some(200));
        assert_eq!(report["props"]["json"]["total_slots"].as_u64(), Some(4));
        assert_eq!(report["summary"]["visible_slots"].as_u64(), Some(2));

        server_handle.join().expect("join mock diag server");

        assert_eq!(
            paths.lock().expect("lock diag paths").as_slice(),
            &["/health", "/props", "/slots"]
        );
    }
}
