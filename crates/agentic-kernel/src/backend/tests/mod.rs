use super::{
    combine_completion_text, completion_is_finished, diagnose_external_backend,
    persist_context_slot_payload_for_backend, resolve_driver_for_family, resolve_driver_for_model,
    runtime_backend_telemetry, BackendClass, CompletionResponse, ContextSlotPersistence,
    ExternalLlamaCppBackend, InferenceBackend, InferenceStepRequest, InferenceStepResult,
    PromptFamily, RuntimeModel, TestExternalEndpointOverrideGuard,
    TestRuntimeDriverAvailabilityGuard,
    TestRemoteOpenAIConfigOverrideGuard,
};
use crate::config::{RemoteAdapterKind, RemoteProviderRuntimeConfig};
use crate::memory::{ContextSlotId, SlotPersistenceKind};
use crate::model_catalog::{RemoteModelEntry, ResolvedModelTarget};
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

fn test_remote_openai_config(endpoint: &str) -> RemoteProviderRuntimeConfig {
    RemoteProviderRuntimeConfig {
        backend_id: "openai-responses".to_string(),
        adapter_kind: RemoteAdapterKind::OpenAICompatible,
        endpoint: endpoint.to_string(),
        api_key: "test-key".to_string(),
        default_model: "gpt-4.1-mini".to_string(),
        timeout_ms: 5_000,
        max_request_bytes: 256 * 1024,
        max_response_bytes: 256 * 1024,
        stream: true,
        tokenizer_path: None,
        input_price_usd_per_mtok: 1.0,
        output_price_usd_per_mtok: 2.0,
        http_referer: String::new(),
        app_title: String::new(),
    }
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
                "/slots/7?action=save" | "/slots/7?action=restore" | "/slots/7?action=erase" => {
                    r#"{"ok":true}"#
                }
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

fn spawn_mock_streaming_tool_server() -> (String, SharedStringLog, SharedStringLog, MockServerHandle)
{
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

fn spawn_mock_openai_responses_server(
) -> (String, SharedStringLog, SharedStringLog, MockServerHandle) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind openai mock server");
    let address = listener.local_addr().expect("openai mock addr");
    let paths = Arc::new(Mutex::new(Vec::new()));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let paths_for_thread = Arc::clone(&paths);
    let bodies_for_thread = Arc::clone(&bodies);

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept openai mock request");
        let mut request_bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let expected_total = loop {
            let read = stream.read(&mut buffer).expect("read openai mock request");
            if read == 0 {
                break request_bytes.len();
            }
            request_bytes.extend_from_slice(&buffer[..read]);

            let Some(header_end) = request_bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request_bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            let total = header_end + content_length;
            if request_bytes.len() >= total {
                break total;
            }
        };
        let request = String::from_utf8_lossy(&request_bytes[..expected_total]);
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
            .expect("lock openai paths")
            .push(path);
        bodies_for_thread
            .lock()
            .expect("lock openai bodies")
            .push(body);

        let chunk_one = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        let chunk_two = "data: {\"type\":\"response.completed\"}\n\n";
        let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                chunk_one.len(),
                chunk_one,
                chunk_two.len(),
                chunk_two,
            );
        let _ = stream.write_all(response.as_bytes());
    });

    (format!("http://{address}/v1"), paths, bodies, handle)
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
    let _driver =
        TestRuntimeDriverAvailabilityGuard::unavailable("test override: runtime unavailable");
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
fn preferred_openai_driver_resolves_unknown_family_when_configured() {
    let _openai = TestRemoteOpenAIConfigOverrideGuard::set(
        "openai-responses",
        test_remote_openai_config("http://127.0.0.1:19090/v1"),
    );

    let resolution = resolve_driver_for_family(PromptFamily::Unknown, Some("openai-responses"))
        .expect("openai backend should resolve unknown family with explicit preference");

    assert_eq!(resolution.resolved_backend_id, "openai-responses");
    assert_eq!(resolution.backend_class, BackendClass::RemoteStateless);
    assert!(resolution.capabilities.structured_output);
    assert!(resolution.available);
    assert!(resolution.load_supported);
}

#[test]
fn preferred_external_driver_errors_when_endpoint_is_missing() {
    let _endpoint = TestExternalEndpointOverrideGuard::clear();
    let _driver =
        TestRuntimeDriverAvailabilityGuard::unavailable("test override: runtime unavailable");
    let err = resolve_driver_for_family(PromptFamily::Qwen, Some("external-llamacpp"))
        .expect_err("external backend should fail when endpoint is missing");
    assert!(err.contains("not loadable"));
}

#[test]
fn unsupported_family_without_loadable_driver_errors() {
    let _endpoint = TestExternalEndpointOverrideGuard::clear();
    let _driver =
        TestRuntimeDriverAvailabilityGuard::unavailable("test override: runtime unavailable");
    let err = resolve_driver_for_family(PromptFamily::Mistral, None)
        .expect_err("mistral should not resolve to loadable driver yet");
    assert!(err.contains("No registered loadable driver"));
}

#[test]
fn architecture_specific_driver_resolution_rejects_qwen35_for_qwen2_backend() {
    let _endpoint = TestExternalEndpointOverrideGuard::clear();
    let _driver =
        TestRuntimeDriverAvailabilityGuard::unavailable("test override: runtime unavailable");
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

#[test]
fn openai_responses_backend_roundtrips_generation() {
    let (endpoint, paths, bodies, server_handle) = spawn_mock_openai_responses_server();
    let _openai = TestRemoteOpenAIConfigOverrideGuard::set(
        "openai-responses",
        test_remote_openai_config(&endpoint),
    );
    super::remote::openai_compatible::reset_telemetry(Some("openai-responses"));

    let mut model = RuntimeModel::load_from_reference(
        "gpt-4.1-mini",
        PromptFamily::Unknown,
        "openai-responses",
    )
    .expect("load openai responses runtime model");
    let tokenizer = test_tokenizer();
    let generation = GenerationConfig {
        temperature: 0.7,
        top_p: 0.9,
        seed: 1,
        max_tokens: 16,
    };

    let step = model
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: "hello",
            resident_prompt_suffix: "hello",
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: None,
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .expect("generate step through openai backend");

    server_handle.join().expect("join openai mock server");

    assert_eq!(model.backend_class(), BackendClass::RemoteStateless);
    assert!(!model.backend_capabilities().resident_kv);
    assert_eq!(step.emitted_text, "hello");
    assert_eq!(step.appended_tokens, vec![1]);
    assert_eq!(
        paths.lock().expect("lock openai paths").as_slice(),
        &["/v1/responses"]
    );
    assert!(
        bodies
            .lock()
            .expect("lock openai bodies")
            .first()
            .map(|body| body.contains("\"model\":\"gpt-4.1-mini\""))
            .unwrap_or(false),
        "openai backend should send the selected remote model id"
    );
    let telemetry =
        runtime_backend_telemetry("openai-responses").expect("openai telemetry available");
    assert_eq!(telemetry.requests_total, 1);
    assert_eq!(telemetry.stream_requests_total, 1);
    assert_eq!(telemetry.input_tokens_total, 1);
    assert_eq!(telemetry.output_tokens_total, 1);
    assert!(telemetry.estimated_cost_usd > 0.0);
    assert_eq!(telemetry.last_model.as_deref(), Some("gpt-4.1-mini"));
}

#[test]
fn groq_responses_backend_roundtrips_generation() {
    let (endpoint, paths, bodies, server_handle) = spawn_mock_openai_responses_server();
    let groq_endpoint = endpoint.replace("/v1", "/openai/v1");
    let _groq = TestRemoteOpenAIConfigOverrideGuard::set(
        "groq-responses",
        test_remote_openai_config(&groq_endpoint),
    );
    super::remote::openai_compatible::reset_telemetry(Some("groq-responses"));

    let mut model = RuntimeModel::load_from_reference(
        "llama-3.3-70b-versatile",
        PromptFamily::Unknown,
        "groq-responses",
    )
    .expect("load groq runtime model");
    let tokenizer = test_tokenizer();
    let generation = GenerationConfig {
        temperature: 0.7,
        top_p: 0.9,
        seed: 1,
        max_tokens: 16,
    };

    let step = model
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: "hello",
            resident_prompt_suffix: "hello",
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: None,
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .expect("generate step through groq backend");

    server_handle.join().expect("join groq mock server");

    assert_eq!(step.emitted_text, "hello");
    assert_eq!(
        paths.lock().expect("lock groq paths").as_slice(),
        &["/openai/v1/responses"]
    );
    assert!(bodies
        .lock()
        .expect("lock groq bodies")
        .first()
        .map(|body| body.contains("\"model\":\"llama-3.3-70b-versatile\""))
        .unwrap_or(false));
    let telemetry = runtime_backend_telemetry("groq-responses").expect("groq telemetry available");
    assert_eq!(telemetry.requests_total, 1);
    assert_eq!(
        telemetry.last_model.as_deref(),
        Some("llama-3.3-70b-versatile")
    );
}

#[test]
fn typed_remote_target_applies_model_specific_limits_pricing_and_capabilities() {
    let (endpoint, _paths, bodies, server_handle) = spawn_mock_openai_responses_server();
    let _openai = TestRemoteOpenAIConfigOverrideGuard::set(
        "openai-responses",
        test_remote_openai_config(&endpoint),
    );
    super::remote::openai_compatible::reset_telemetry(Some("openai-responses"));
    let driver_resolution =
        resolve_driver_for_model(PromptFamily::Unknown, None, Some("openai-responses"))
            .expect("resolve openai backend");
    let target = ResolvedModelTarget::remote(
        "openai-responses",
        "OpenAI",
        "openai-responses",
        "gpt-4.1-mini",
        RemoteModelEntry {
            id: "gpt-4.1-mini".to_string(),
            label: "GPT-4.1 mini".to_string(),
            context_window_tokens: Some(1_024),
            max_output_tokens: Some(8),
            supports_structured_output: false,
            input_price_usd_per_mtok: Some(10.0),
            output_price_usd_per_mtok: Some(20.0),
        },
        test_remote_openai_config(&endpoint),
        None,
        driver_resolution,
    );

    let mut model = RuntimeModel::load_target(&target).expect("load typed remote target");
    assert!(!model.backend_capabilities().structured_output);

    let tokenizer = test_tokenizer();
    let generation = GenerationConfig {
        temperature: 0.7,
        top_p: 0.9,
        seed: 1,
        max_tokens: 16,
    };

    let step = model
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: "hello",
            resident_prompt_suffix: "hello",
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: None,
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .expect("generate step through typed remote target");

    server_handle.join().expect("join openai mock server");

    assert_eq!(step.emitted_text, "hello");
    assert!(
        bodies
            .lock()
            .expect("lock openai bodies")
            .first()
            .map(|body| body.contains("\"max_output_tokens\":8"))
            .unwrap_or(false),
        "typed remote target should clamp request max_output_tokens to model metadata"
    );
    let telemetry =
        runtime_backend_telemetry("openai-responses").expect("openai telemetry available");
    assert!((telemetry.estimated_cost_usd - 0.00003).abs() < 1e-9);
}

#[test]
fn openrouter_backend_uses_chat_completions_transport() {
    let (endpoint, paths, bodies, server_handle) = spawn_mock_openai_responses_server();
    let openrouter_endpoint = endpoint.replace("/v1", "/api/v1");
    let _openrouter = TestRemoteOpenAIConfigOverrideGuard::set(
        "openrouter",
        RemoteProviderRuntimeConfig {
            backend_id: "openrouter".to_string(),
            adapter_kind: RemoteAdapterKind::OpenAICompatible,
            endpoint: openrouter_endpoint,
            api_key: "test-key".to_string(),
            default_model: "openai/gpt-4.1-mini".to_string(),
            timeout_ms: 5_000,
            max_request_bytes: 256 * 1024,
            max_response_bytes: 256 * 1024,
            stream: true,
            tokenizer_path: None,
            input_price_usd_per_mtok: 0.0,
            output_price_usd_per_mtok: 0.0,
            http_referer: "https://agenticos.local".to_string(),
            app_title: "AgenticOS".to_string(),
        },
    );
    super::remote::openai_compatible::reset_telemetry(Some("openrouter"));

    let mut model = RuntimeModel::load_from_reference(
        "openai/gpt-4.1-mini",
        PromptFamily::Unknown,
        "openrouter",
    )
    .expect("load openrouter runtime model");
    let tokenizer = test_tokenizer();
    let generation = GenerationConfig {
        temperature: 0.7,
        top_p: 0.9,
        seed: 1,
        max_tokens: 16,
    };

    let _ = model
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: "hello",
            resident_prompt_suffix: "hello",
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: None,
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .expect("generate step through openrouter backend");

    server_handle.join().expect("join openrouter mock server");

    assert_eq!(
        paths.lock().expect("lock openrouter paths").as_slice(),
        &["/api/v1/chat/completions"]
    );
    let body = bodies
        .lock()
        .expect("lock openrouter bodies")
        .first()
        .cloned()
        .unwrap_or_default();
    assert!(body.contains("\"prompt\":\"hello\""));
    assert!(body.contains("\"model\":\"openai/gpt-4.1-mini\""));
}

struct DummyBackend;

impl InferenceBackend for DummyBackend {
    fn backend_id(&self) -> &'static str {
        "dummy"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Unknown
    }

    fn generate_step(&mut self, _request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
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

    fn generate_step(&mut self, _request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
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
            stream_observer: None,
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
            stream_observer: None,
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
        persist_context_slot_payload_for_backend(
            "external-llamacpp",
            PromptFamily::Qwen,
            7,
            &final_path,
        )
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
            stream_observer: None,
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
            stream_observer: None,
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
            stream_observer: None,
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
        body.contains(
            "\"prompt\":\"hello\\nTOOL:calc {\\\"expression\\\":\\\"1+1\\\"}\\nOutput:\\n2\\n\""
        ),
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
            stream_observer: None,
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
