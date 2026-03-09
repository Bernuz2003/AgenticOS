use anyhow::{Error as E, Result};
use candle_core::Device;
use candle_transformers::generation::LogitsProcessor;
use std::fs;
use std::io::Write;
use std::path::Path;
use tokenizers::Tokenizer;

use crate::memory::ContextSlotId;
use crate::prompting::{GenerationConfig, PromptFamily};

mod external_llamacpp;
mod http;
mod local;
mod diagnostics;
mod remote_adapter;

pub(crate) use diagnostics::diagnose_external_backend;
use external_llamacpp::ExternalLlamaCppBackend;
use local::{QuantizedLlamaBackend, QuantizedQwen2Backend};

#[cfg(test)]
use remote_adapter::{
    combine_completion_text, completion_is_finished, CompletionResponse,
};

pub struct DriverDescriptor {
    pub id: &'static str,
    pub kind: &'static str,
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
            || self
                .architectures
                .iter()
                .any(|candidate| architecture.is_some_and(|arch| candidate.eq_ignore_ascii_case(arch)))
    }

    fn supports_model(&self, family: PromptFamily, architecture: Option<&str>) -> bool {
        self.supports_family(family) && self.supports_architecture(architecture)
    }
}

#[derive(Debug, Clone)]
pub struct DriverResolution {
    pub resolved_backend_id: String,
    pub resolution_source: &'static str,
    pub resolution_rationale: String,
    pub available: bool,
    pub load_supported: bool,
}

const FAMILIES_LLAMA: [PromptFamily; 1] = [PromptFamily::Llama];
const FAMILIES_QWEN: [PromptFamily; 1] = [PromptFamily::Qwen];
const FAMILIES_COMMON: [PromptFamily; 3] = [PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral];
const ARCH_LLAMA: [&str; 1] = ["llama"];
const ARCH_QWEN2: [&str; 1] = ["qwen2"];
const ARCH_ANY: [&str; 0] = [];

const DRIVER_REGISTRY: [DriverDescriptor; 3] = [
    DriverDescriptor {
        id: "candle.quantized_llama",
        kind: "internal",
        available: true,
        load_supported: true,
        note: "Built-in Candle quantized Llama backend.",
        families: &FAMILIES_LLAMA,
        architectures: &ARCH_LLAMA,
    },
    DriverDescriptor {
        id: "candle.quantized_qwen2",
        kind: "internal",
        available: true,
        load_supported: true,
        note: "Built-in Candle quantized Qwen2 backend.",
        families: &FAMILIES_QWEN,
        architectures: &ARCH_QWEN2,
    },
    DriverDescriptor {
        id: "external-llamacpp",
        kind: "external-stub",
        available: false,
        load_supported: false,
        note: "Reserved external driver slot for future llama.cpp integration.",
        families: &FAMILIES_COMMON,
        architectures: &ARCH_ANY,
    },
];

pub fn driver_registry() -> &'static [DriverDescriptor] {
    &DRIVER_REGISTRY
}

fn external_llamacpp_endpoint() -> Option<String> {
    #[cfg(test)]
    {
        return test_external_endpoint_override_get();
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
thread_local! {
    static TEST_EXTERNAL_ENDPOINT_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_external_endpoint_override_get() -> Option<String> {
    TEST_EXTERNAL_ENDPOINT_OVERRIDE.with(|cell| cell.borrow().clone())
}

#[cfg(test)]
fn test_external_endpoint_override_set(value: Option<String>) {
    TEST_EXTERNAL_ENDPOINT_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = value;
    });
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

pub(crate) fn persist_context_slot_payload_for_backend(
    backend_id: &str,
    _slot_id: ContextSlotId,
    tmp_path: &Path,
    final_path: &Path,
    payload: &[u8],
) -> Result<(), String> {
    match backend_id {
        "candle.quantized_llama" | "candle.quantized_qwen2" | "candle.slot-compat" => {
            persist_candle_slot_payload(tmp_path, final_path, payload)
        }
        other => Err(format!(
            "Backend '{}' does not yet expose a physical context-slot save mechanism.",
            other
        )),
    }
}

fn persist_candle_slot_payload(
    tmp_path: &Path,
    final_path: &Path,
    payload: &[u8],
) -> Result<(), String> {
    let Some(base_dir) = final_path.parent() else {
        return Err("Swap path safety violation: final path has no parent directory".to_string());
    };

    if tmp_path.parent() != Some(base_dir) || final_path.parent() != Some(base_dir) {
        return Err("Swap path safety violation: computed file path escaped base dir".to_string());
    }

    let mut tmp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp_path)
        .map_err(|e| format!("tmp open failed: {}", e))?;

    if let Err(e) = tmp_file.write_all(payload) {
        let _ = fs::remove_file(tmp_path);
        return Err(format!("tmp write failed: {}", e));
    }

    if let Err(e) = tmp_file.sync_all() {
        let _ = fs::remove_file(tmp_path);
        return Err(format!("tmp fsync failed: {}", e));
    }

    drop(tmp_file);

    if let Err(e) = fs::rename(tmp_path, final_path) {
        let _ = fs::remove_file(tmp_path);
        return Err(format!("atomic rename failed: {}", e));
    }

    Ok(())
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

    let fallback = DRIVER_REGISTRY
        .iter()
        .find(|driver| driver.supports_model(family, architecture) && is_driver_runtime_loadable(driver));

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
    fn generate_step(
        &mut self,
        context_slot_id: Option<ContextSlotId>,
        tokens: &[u32],
        index_pos: usize,
        logits_processor: &mut LogitsProcessor,
        tokenizer: &Tokenizer,
        generation: GenerationConfig,
        device: &Device,
        eos_token_id: u32,
        eot_token_id: u32,
    ) -> Result<InferenceStepResult>;
    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>>;
}

#[derive(Debug, Clone)]
pub struct InferenceStepResult {
    pub appended_tokens: Vec<u32>,
    pub emitted_text: String,
    pub finished: bool,
    pub next_index_pos: usize,
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

    pub fn load_from_gguf(
        path: &str,
        family: PromptFamily,
        backend_id: &str,
        device: &Device,
    ) -> Result<Self> {
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

        if backend_id != "external-llamacpp" && (!descriptor.available || !descriptor.load_supported) {
            return Err(E::msg(format!(
                "Backend '{}' is registered as '{}' but is not loadable in-process yet: {}",
                backend_id, descriptor.kind, descriptor.note
            )));
        }

        let backend: Box<dyn ModelBackend> = match backend_id {
            "candle.quantized_llama" => Box::new(QuantizedLlamaBackend::load(path, device)?),
            "candle.quantized_qwen2" => Box::new(QuantizedQwen2Backend::load(path, device)?),
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

    pub fn generate_step(
        &mut self,
        context_slot_id: Option<ContextSlotId>,
        tokens: &[u32],
        index_pos: usize,
        logits_processor: &mut LogitsProcessor,
        tokenizer: &Tokenizer,
        generation: GenerationConfig,
        device: &Device,
        eos_token_id: u32,
        eot_token_id: u32,
    ) -> Result<InferenceStepResult> {
        self.inner.generate_step(
            context_slot_id,
            tokens,
            index_pos,
            logits_processor,
            tokenizer,
            generation,
            device,
            eos_token_id,
            eot_token_id,
        )
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
        self.inner
            .duplicate_boxed()
            .map(|inner| Self { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        combine_completion_text, completion_is_finished, diagnose_external_backend,
        resolve_driver_for_family, resolve_driver_for_model, test_external_endpoint_override_get,
        test_external_endpoint_override_set, CompletionResponse, ContextSlotPersistence,
        ExternalLlamaCppBackend,
        InferenceBackend, InferenceStepResult, PromptFamily, RuntimeModel,
    };
    use crate::memory::ContextSlotId;
    use crate::prompting::GenerationConfig;
    use anyhow::Result;
    use candle_core::Device;
    use candle_transformers::generation::LogitsProcessor;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::Tokenizer;

    struct EndpointOverrideGuard {
        previous: Option<String>,
    }

    impl EndpointOverrideGuard {
        fn set(value: &str) -> Self {
            let previous = test_external_endpoint_override_get();
            test_external_endpoint_override_set(Some(value.to_string()));
            Self { previous }
        }
    }

    impl Drop for EndpointOverrideGuard {
        fn drop(&mut self) {
            test_external_endpoint_override_set(self.previous.clone());
        }
    }

    fn test_tokenizer() -> Tokenizer {
        let vocab = [
            ("<unk>".to_string(), 0),
            ("hello".to_string(), 1),
        ]
        .into_iter()
        .collect();

        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build wordlevel tokenizer");

        Tokenizer::new(model)
    }

    fn spawn_mock_llamacpp_server(expected_requests: usize) -> (String, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
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
                paths_for_thread.lock().expect("lock paths").push(path.clone());
                bodies_for_thread.lock().expect("lock bodies").push(body);

                let body = match path.as_str() {
                    "/completion" => r#"{"content":"hello","tokens":[1]}"#,
                    "/slots/7?action=save" | "/slots/7?action=restore" | "/slots/7?action=erase" => r#"{"ok":true}"#,
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

    fn spawn_mock_diag_server() -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
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
                paths_for_thread.lock().expect("lock diag paths").push(path.clone());

                let (status, body) = match path.as_str() {
                    "/health" => ("HTTP/1.1 200 OK", r#"{"status":"ok"}"#),
                    "/props" => ("HTTP/1.1 200 OK", r#"{"model_path":"/models/qwen3.5.gguf","total_slots":4}"#),
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
    fn resolves_family_default_driver() {
        let resolution =
            resolve_driver_for_family(PromptFamily::Llama, None).expect("resolve llama driver");
        assert_eq!(resolution.resolved_backend_id, "candle.quantized_llama");
        assert_eq!(resolution.resolution_source, "family-default");
    }

    #[test]
    fn preferred_external_driver_falls_back_when_stub_only() {
        let resolution = resolve_driver_for_family(
            PromptFamily::Qwen,
            Some("external-llamacpp"),
        )
        .expect("resolve qwen fallback driver");
        assert_eq!(resolution.resolved_backend_id, "candle.quantized_qwen2");
        assert_eq!(resolution.resolution_source, "metadata-preference-fallback");
    }

    #[test]
    fn unsupported_family_without_loadable_driver_errors() {
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
        let _endpoint = EndpointOverrideGuard::set("http://127.0.0.1:18080");

        let resolution = resolve_driver_for_model(PromptFamily::Qwen, Some("qwen35"), None)
            .expect("qwen35 should resolve to external rpc when configured");

        assert_eq!(resolution.resolved_backend_id, "external-llamacpp");
        assert_eq!(resolution.resolution_source, "family-default");
        assert!(resolution.available);
        assert!(resolution.load_supported);
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
            _context_slot_id: Option<ContextSlotId>,
            _tokens: &[u32],
            _index_pos: usize,
            _logits_processor: &mut LogitsProcessor,
            _tokenizer: &Tokenizer,
            _generation: GenerationConfig,
            _device: &Device,
            _eos_token_id: u32,
            _eot_token_id: u32,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in this test");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn super::ModelBackend>> {
            None
        }
    }

    impl ContextSlotPersistence for DummyBackend {
    }

    #[test]
    fn runtime_model_exposes_context_slot_boundary_with_unsupported_default() {
        let mut model = RuntimeModel::from_boxed_backend(Box::new(DummyBackend));

        let save_err = model
            .save_context_slot(ContextSlotId::from(7_u64), Path::new("workspace/swap/pid_7.swap"))
            .expect_err("default context slot persistence should be unsupported");
        let load_err = model
            .load_context_slot(ContextSlotId::from(7_u64), Path::new("workspace/swap/pid_7.swap"))
            .expect_err("default context slot load should be unsupported");
        let free_err = model
            .free_context_slot(ContextSlotId::from(7_u64))
            .expect_err("default context slot free should be unsupported");

        assert!(save_err.to_string().contains("does not yet support saving context slot 7"));
        assert!(load_err.to_string().contains("does not yet support loading context slot 7"));
        assert!(free_err.to_string().contains("does not yet support freeing context slot 7"));
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
            _context_slot_id: Option<ContextSlotId>,
            _tokens: &[u32],
            _index_pos: usize,
            _logits_processor: &mut LogitsProcessor,
            _tokenizer: &Tokenizer,
            _generation: GenerationConfig,
            _device: &Device,
            _eos_token_id: u32,
            _eot_token_id: u32,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in this test");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn super::ModelBackend>> {
            None
        }
    }

    impl ContextSlotPersistence for RecordingBackend {
        fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
            self.freed_slots.lock().expect("lock freed slots").push(slot_id);
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
        let _endpoint = EndpointOverrideGuard::set(&endpoint);

        let mut model = RuntimeModel::load_from_gguf(
            "ignored.gguf",
            PromptFamily::Qwen,
            "external-llamacpp",
            &Device::Cpu,
        )
        .expect("load external runtime model");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 16,
        };
        let mut logits_processor = LogitsProcessor::new(1, Some(0.7), Some(0.9));

        let step = model
            .generate_step(
                Some(7),
                &[1],
                0,
                &mut logits_processor,
                &tokenizer,
                generation,
                &Device::Cpu,
                2,
                3,
            )
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
    fn external_backend_preserves_special_tokens_in_prompt_decode() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = EndpointOverrideGuard::set(&endpoint);

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
        let mut logits_processor = LogitsProcessor::new(1, Some(0.7), Some(0.9));

        backend
            .generate_step(
                Some(3),
                &[1, 2, 3, 4, 5],
                0,
                &mut logits_processor,
                &tokenizer,
                generation,
                &Device::Cpu,
                6,
                7,
            )
            .expect("generate step should succeed");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(body.contains("<|im_start|>"), "special chat tokens must survive prompt decode");
        assert!(body.contains("<|im_end|>"), "end markers must survive prompt decode");
    }

    #[test]
    fn external_backend_uses_chunked_completion_requests() {
        let (endpoint, _paths, bodies, server_handle) = spawn_mock_llamacpp_server(1);
        let _endpoint = EndpointOverrideGuard::set(&endpoint);

        let mut backend = ExternalLlamaCppBackend::from_env(PromptFamily::Qwen)
            .expect("build external backend from endpoint override");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 64,
        };
        let mut logits_processor = LogitsProcessor::new(1, Some(0.7), Some(0.9));

        backend
            .generate_step(
                Some(3),
                &[1],
                0,
                &mut logits_processor,
                &tokenizer,
                generation,
                &Device::Cpu,
                6,
                7,
            )
            .expect("generate step should succeed");

        server_handle.join().expect("join mock server");

        let body = bodies
            .lock()
            .expect("lock bodies")
            .first()
            .cloned()
            .unwrap_or_default();
        assert!(body.contains("\"n_predict\":32"), "external backend should request a larger completion chunk");
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

        assert_eq!(
            emitted_text,
            "<think>\nStep 1: add 2 and 2.\n</think>\n4"
        );
    }

    #[test]
    fn external_backend_diagnostic_reports_health_props_and_slots() {
        let (endpoint, paths, server_handle) = spawn_mock_diag_server();
        let _endpoint = EndpointOverrideGuard::set(&endpoint);

        let report = diagnose_external_backend().expect("diagnostic report should succeed");

        assert_eq!(report["backend"].as_str(), Some("external-llamacpp"));
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
