use super::{RuntimeRegistry, RuntimeReservation};
use crate::backend::{
    resolve_driver_for_model, BackendClass, TestExternalEndpointOverrideGuard,
    TestOpenAIConfigOverrideGuard,
};
use crate::config::OpenAIResponsesConfig;
use crate::memory::NeuralMemory;
use crate::model_catalog::{RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
use crate::process::ProcessLifecyclePolicy;
use crate::prompting::PromptFamily;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tools::invocation::ToolCaller;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenizers::models::wordlevel::WordLevel;
use tokenizers::Tokenizer;

#[test]
fn parallel_sessions_can_bind_to_different_runtime_backends() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
    let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
    let dir = make_temp_dir("agenticos-runtime-registry");
    let db_path = dir.join("agenticos.db");
    let tokenizer_path = write_test_tokenizer();

    let mut storage = StorageService::open(&db_path).expect("open storage");
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");
    let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load runtimes");
    let mut session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let remote_runtime = runtime_registry
        .activate_target(
            &mut storage,
            &remote_target(),
            RuntimeReservation::default(),
        )
        .expect("activate remote runtime");
    let local_runtime = runtime_registry
        .activate_target(
            &mut storage,
            &local_target(&tokenizer_path),
            RuntimeReservation {
                ram_bytes: 1,
                vram_bytes: 1,
            },
        )
        .expect("activate local runtime");

    let remote_spawn = {
        let pid_floor = runtime_registry.next_pid_floor();
        let engine = runtime_registry
            .engine_mut(&remote_runtime.runtime_id)
            .expect("remote engine");
        spawn_managed_process_with_session(
            &remote_runtime.runtime_id,
            pid_floor,
            engine,
            &mut memory,
            &mut scheduler,
            &mut session_registry,
            &mut storage,
            ManagedProcessRequest {
                prompt: "ping remote backend".to_string(),
                system_prompt: None,
                owner_id: 10,
                tool_caller: ToolCaller::AgentText,
                workload: WorkloadClass::Fast,
                required_backend_class: Some(BackendClass::RemoteStateless),
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
            },
        )
        .expect("spawn remote session")
    };
    runtime_registry
        .register_pid(&mut storage, &remote_runtime.runtime_id, remote_spawn.pid)
        .expect("register remote pid");

    let local_spawn = {
        let pid_floor = runtime_registry.next_pid_floor();
        let engine = runtime_registry
            .engine_mut(&local_runtime.runtime_id)
            .expect("local engine");
        spawn_managed_process_with_session(
            &local_runtime.runtime_id,
            pid_floor,
            engine,
            &mut memory,
            &mut scheduler,
            &mut session_registry,
            &mut storage,
            ManagedProcessRequest {
                prompt: "ping local backend".to_string(),
                system_prompt: None,
                owner_id: 11,
                tool_caller: ToolCaller::AgentText,
                workload: WorkloadClass::General,
                required_backend_class: Some(BackendClass::ResidentLocal),
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
            },
        )
        .expect("spawn local session")
    };
    runtime_registry
        .register_pid(&mut storage, &local_runtime.runtime_id, local_spawn.pid)
        .expect("register local pid");

    assert_ne!(remote_runtime.runtime_id, local_runtime.runtime_id);
    assert_eq!(runtime_registry.runtime_count(), 2);
    assert_eq!(runtime_registry.live_process_count(), 2);
    assert_eq!(
        session_registry.runtime_id_for_pid(remote_spawn.pid),
        Some(remote_runtime.runtime_id.as_str())
    );
    assert_eq!(
        session_registry.runtime_id_for_pid(local_spawn.pid),
        Some(local_runtime.runtime_id.as_str())
    );
    assert_eq!(
        runtime_registry
            .descriptor(&remote_runtime.runtime_id)
            .expect("remote descriptor")
            .backend_class,
        BackendClass::RemoteStateless
    );
    assert_eq!(
        runtime_registry
            .descriptor(&local_runtime.runtime_id)
            .expect("local descriptor")
            .backend_class,
        BackendClass::ResidentLocal
    );

    let _ = fs::remove_file(tokenizer_path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn session_title_uses_user_prompt_not_system_prompt() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
    let dir = make_temp_dir("agenticos-runtime-registry-title");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");
    let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load runtimes");
    let mut session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
    let mut memory = NeuralMemory::new().expect("memory init");
    let mut scheduler = ProcessScheduler::new();

    let runtime = runtime_registry
        .activate_target(
            &mut storage,
            &remote_target(),
            RuntimeReservation::default(),
        )
        .expect("activate remote runtime");

    let spawned = {
        let pid_floor = runtime_registry.next_pid_floor();
        let engine = runtime_registry
            .engine_mut(&runtime.runtime_id)
            .expect("remote engine");
        spawn_managed_process_with_session(
            &runtime.runtime_id,
            pid_floor,
            engine,
            &mut memory,
            &mut scheduler,
            &mut session_registry,
            &mut storage,
            ManagedProcessRequest {
                prompt: "Solve the task".to_string(),
                system_prompt: Some("Tool syntax: TOOL:<name> <json-object>.".to_string()),
                owner_id: 10,
                tool_caller: ToolCaller::AgentText,
                workload: WorkloadClass::Fast,
                required_backend_class: Some(BackendClass::RemoteStateless),
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
            },
        )
        .expect("spawn remote session")
    };

    let session = session_registry
        .session(&spawned.session_id)
        .expect("session should exist");
    assert!(session.title.contains("Solve the task"));
    assert!(!session.title.contains("Tool syntax"));

    let _ = fs::remove_dir_all(dir);
}

fn remote_target() -> ResolvedModelTarget {
    let driver_resolution =
        resolve_driver_for_model(PromptFamily::Unknown, None, Some("openai-responses"))
            .expect("resolve remote backend");
    ResolvedModelTarget::remote(
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
    )
}

fn local_target(tokenizer_path: &Path) -> ResolvedModelTarget {
    let driver_resolution =
        resolve_driver_for_model(PromptFamily::Mistral, None, Some("external-llamacpp"))
            .expect("resolve local backend");
    ResolvedModelTarget::local(
        None,
        PathBuf::from("ignored.gguf"),
        PromptFamily::Mistral,
        Some(tokenizer_path.to_path_buf()),
        None,
        driver_resolution,
    )
}

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

fn write_test_tokenizer() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("agenticos-runtime-registry-{unique}.json"));
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
        .expect("build tokenizer");
    Tokenizer::new(model)
        .save(&path, false)
        .expect("save tokenizer");
    path
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), unique));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
