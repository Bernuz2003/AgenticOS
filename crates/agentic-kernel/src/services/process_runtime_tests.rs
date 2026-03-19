use super::{
    spawn_managed_process, spawn_restored_managed_process, ManagedProcessRequest,
    RestoredManagedProcessRequest,
};
use crate::backend::{
    resolve_driver_for_model, BackendClass, TestExternalEndpointOverrideGuard,
    TestOpenAIConfigOverrideGuard,
};
use crate::config::OpenAIResponsesConfig;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::{RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
use crate::process::ProcessLifecyclePolicy;
use crate::prompting::PromptFamily;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::tools::invocation::ToolCaller;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenizers::models::wordlevel::WordLevel;
use tokenizers::Tokenizer;

fn test_openai_config() -> OpenAIResponsesConfig {
    OpenAIResponsesConfig {
        endpoint: "http://127.0.0.1:19090/v1".to_string(),
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

fn test_tokenizer() -> Tokenizer {
    let vocab = [
        ("<unk>".to_string(), 0),
        ("hello".to_string(), 1),
        ("</s>".to_string(), 2),
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

fn write_test_tokenizer() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("agenticos-process-runtime-{unique}.json"));
    test_tokenizer().save(&path, false).expect("save tokenizer");
    path
}

#[test]
fn remote_stateless_processes_skip_resident_slot_binding() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
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
            context_window_tokens: None,
            max_output_tokens: None,
            supports_structured_output: true,
            input_price_usd_per_mtok: None,
            output_price_usd_per_mtok: None,
        },
        test_openai_config().into(),
        None,
        driver_resolution,
    );

    let mut engine = LLMEngine::load_target(&target).expect("load remote stateless engine");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let spawned = spawn_managed_process(
        &mut engine,
        &mut memory,
        &mut scheduler,
        ManagedProcessRequest {
            prompt: "ping cloud backend".to_string(),
            system_prompt: None,
            owner_id: 7,
            tool_caller: ToolCaller::AgentText,
            workload: WorkloadClass::Fast,
            required_backend_class: Some(BackendClass::RemoteStateless),
            priority: ProcessPriority::Normal,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: None,
        },
    )
    .expect("spawn managed process");

    let process = engine
        .processes
        .get(&spawned.pid)
        .expect("spawned process present");

    assert_eq!(engine.loaded_backend_class().as_str(), "remote_stateless");
    assert_eq!(process.context_slot_id, None);
    assert_eq!(process.resident_slot_policy_label(), None);
    assert_eq!(process.resident_slot_state_label(), None);
    assert_eq!(memory.slot_for_pid(spawned.pid), None);
}

#[test]
fn remote_stateless_engine_rejects_resident_local_task_policy() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
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
            context_window_tokens: None,
            max_output_tokens: None,
            supports_structured_output: true,
            input_price_usd_per_mtok: None,
            output_price_usd_per_mtok: None,
        },
        test_openai_config().into(),
        None,
        driver_resolution,
    );

    let mut engine = LLMEngine::load_target(&target).expect("load remote stateless engine");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let err = spawn_managed_process(
        &mut engine,
        &mut memory,
        &mut scheduler,
        ManagedProcessRequest {
            prompt: "this task expects residency".to_string(),
            system_prompt: None,
            owner_id: 7,
            tool_caller: ToolCaller::AgentText,
            workload: WorkloadClass::Code,
            required_backend_class: Some(BackendClass::ResidentLocal),
            priority: ProcessPriority::Normal,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: None,
        },
    )
    .expect_err("remote stateless engine should reject resident-local routing");

    assert!(err.contains("resident_local"));
    assert!(err.contains("remote_stateless"));
}

#[test]
fn resident_local_engine_rejects_remote_stateless_task_policy() {
    let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
    let tokenizer_path = write_test_tokenizer();
    let driver_resolution =
        resolve_driver_for_model(PromptFamily::Mistral, None, Some("external-llamacpp"))
            .expect("resolve resident backend");
    let target = ResolvedModelTarget::local(
        None,
        PathBuf::from("ignored.gguf"),
        PromptFamily::Mistral,
        Some(tokenizer_path.clone()),
        None,
        driver_resolution,
    );

    let mut engine = LLMEngine::load_target(&target).expect("load resident local engine");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let err = spawn_managed_process(
        &mut engine,
        &mut memory,
        &mut scheduler,
        ManagedProcessRequest {
            prompt: "this task expects cloud execution".to_string(),
            system_prompt: None,
            owner_id: 7,
            tool_caller: ToolCaller::AgentText,
            workload: WorkloadClass::Fast,
            required_backend_class: Some(BackendClass::RemoteStateless),
            priority: ProcessPriority::Normal,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: None,
        },
    )
    .expect_err("resident local engine should reject remote-stateless routing");

    let _ = std::fs::remove_file(tokenizer_path);

    assert!(err.contains("remote_stateless"));
    assert!(err.contains("resident_local"));
}

#[test]
fn spawn_managed_process_injects_system_prompt_without_losing_user_prompt() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
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
            context_window_tokens: None,
            max_output_tokens: None,
            supports_structured_output: true,
            input_price_usd_per_mtok: None,
            output_price_usd_per_mtok: None,
        },
        test_openai_config().into(),
        None,
        driver_resolution,
    );

    let mut engine = LLMEngine::load_target(&target).expect("load remote stateless engine");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let spawned = spawn_managed_process(
        &mut engine,
        &mut memory,
        &mut scheduler,
        ManagedProcessRequest {
            prompt: "user task".to_string(),
            system_prompt: Some("kernel policy".to_string()),
            owner_id: 7,
            tool_caller: ToolCaller::AgentText,
            workload: WorkloadClass::Fast,
            required_backend_class: Some(BackendClass::RemoteStateless),
            priority: ProcessPriority::Normal,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: None,
        },
    )
    .expect("spawn managed process");

    let process = engine
        .processes
        .get(&spawned.pid)
        .expect("spawned process present");

    assert!(process.prompt_text().contains("kernel policy"));
    assert!(process.prompt_text().contains("user task"));
}

#[test]
fn restored_managed_process_starts_waiting_for_input_with_persisted_prompt_cache() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
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
            context_window_tokens: None,
            max_output_tokens: None,
            supports_structured_output: true,
            input_price_usd_per_mtok: None,
            output_price_usd_per_mtok: None,
        },
        test_openai_config().into(),
        None,
        driver_resolution,
    );

    let mut engine = LLMEngine::load_target(&target).expect("load remote stateless engine");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let spawned = spawn_restored_managed_process(
        &mut engine,
        &mut memory,
        &mut scheduler,
        RestoredManagedProcessRequest {
            rendered_prompt: "system\nuser hello\nassistant world".to_string(),
            owner_id: 7,
            tool_caller: ToolCaller::AgentText,
            workload: WorkloadClass::General,
            required_backend_class: Some(BackendClass::RemoteStateless),
            priority: ProcessPriority::Normal,
            lifecycle_policy: ProcessLifecyclePolicy::Interactive,
            context_policy: None,
        },
    )
    .expect("spawn restored managed process");

    let process = engine
        .processes
        .get(&spawned.pid)
        .expect("restored process present");

    assert_eq!(process.state, crate::process::ProcessState::WaitingForInput);
    assert!(process.prompt_text().contains("user hello"));
    assert!(process.prompt_text().contains("assistant world"));
}
