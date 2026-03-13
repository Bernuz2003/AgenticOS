use crate::audit::{self, AuditContext};
use crate::backend::BackendClass;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::process::{ContextPolicy, ProcessLifecyclePolicy};
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::session::SessionRegistry;
use crate::storage::StorageService;

#[derive(Debug)]
pub struct ManagedProcessSpawn {
    pub session_id: String,
    pub runtime_id: String,
    pub pid: u64,
}

pub struct ManagedProcessRequest {
    pub prompt: String,
    pub owner_id: usize,
    pub workload: WorkloadClass,
    pub required_backend_class: Option<BackendClass>,
    pub priority: ProcessPriority,
    pub lifecycle_policy: ProcessLifecyclePolicy,
    pub context_policy: Option<ContextPolicy>,
}

pub fn free_backend_slot_if_known(engine: &mut LLMEngine, memory: &NeuralMemory, pid: u64) {
    if let Err(err) = engine.free_process_context_slot(pid) {
        let Some(slot_id) = memory.slot_for_pid(pid) else {
            return;
        };

        if let Err(fallback_err) = engine.free_context_slot(slot_id) {
            tracing::debug!(
                pid,
                slot_id,
                primary_error = %err,
                fallback_error = %fallback_err,
                "MEMORY: backend slot free not available"
            );
        }
    }
}

pub fn release_process_resources(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
) {
    free_backend_slot_if_known(engine, memory, pid);
    let _ = memory.release_process(pid);
    scheduler.unregister(pid);
}

pub fn release_process_resources_with_session(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pid: u64,
    run_state: &str,
) {
    if let Err(err) = session_registry.release_pid(storage, pid, run_state) {
        tracing::warn!(pid, %err, "PROCESS_RUNTIME: failed to release session binding");
    }
    release_process_resources(engine, memory, scheduler, pid);
}

pub fn kill_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
) {
    release_process_resources(engine, memory, scheduler, pid);
    engine.kill_process(pid);
}

pub fn kill_managed_process_with_session(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pid: u64,
    run_state: &str,
) {
    release_process_resources_with_session(
        engine,
        memory,
        scheduler,
        session_registry,
        storage,
        pid,
        run_state,
    );
    engine.kill_process(pid);
}

pub fn spawn_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    request: ManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    validate_backend_class_policy(
        engine.loaded_backend_class(),
        request.required_backend_class,
    )?;

    let context_policy = request
        .context_policy
        .unwrap_or_else(ContextPolicy::from_kernel_defaults);
    let pid = engine
        .spawn_process(
            &request.prompt,
            0,
            request.owner_id,
            request.lifecycle_policy,
            context_policy,
        )
        .map_err(|e| e.to_string())?;

    let backend_capabilities = engine.loaded_backend_capabilities();
    let should_bind_resident_slot = backend_capabilities.resident_kv
        || backend_capabilities.persistent_slots
        || backend_capabilities.save_restore_slots;

    if should_bind_resident_slot {
        if let Some(token_slots) = engine.process_max_tokens(pid) {
            match memory.register_process(pid, token_slots) {
                Ok(slot_id) => {
                    if let Err(err) = engine.set_process_context_slot(pid, slot_id) {
                        let _ = memory.release_process(pid);
                        engine.kill_process(pid);
                        return Err(err.to_string());
                    }
                }
                Err(err) => {
                    engine.kill_process(pid);
                    return Err(err.to_string());
                }
            }
        }
    } else {
        tracing::info!(
            pid,
            backend_class = engine.loaded_backend_class().as_str(),
            "PROCESS_RUNTIME: skipping resident slot allocation for non-resident backend"
        );
    }

    scheduler.register(pid, request.workload, request.priority);
    Ok(ManagedProcessSpawn {
        session_id: format!("pid-{pid}"),
        runtime_id: String::new(),
        pid,
    })
}

pub fn spawn_managed_process_with_session(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    request: ManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    engine.ensure_next_pid_at_least(pid_floor);
    let session_id = session_registry
        .open_session(storage, &request.prompt, runtime_id)
        .map_err(|err| err.to_string())?;
    let request_workload = request.workload;
    let request_lifecycle = request.lifecycle_policy;

    match spawn_managed_process(engine, memory, scheduler, request) {
        Ok(mut spawned) => {
            if let Err(err) =
                session_registry.bind_pid(storage, &session_id, runtime_id, spawned.pid)
            {
                kill_managed_process(engine, memory, scheduler, spawned.pid);
                if let Err(cleanup_err) = session_registry.delete_session(storage, &session_id) {
                    tracing::warn!(
                        session_id,
                        pid = spawned.pid,
                        error = %cleanup_err,
                        "PROCESS_RUNTIME: failed to clean up session after bind failure"
                    );
                }
                return Err(err.to_string());
            }

            spawned.session_id = session_id;
            spawned.runtime_id = runtime_id.to_string();
            audit::record(
                storage,
                audit::PROCESS_SPAWNED,
                format!(
                    "pid={} runtime={} workload={:?} lifecycle={:?}",
                    spawned.pid, runtime_id, request_workload, request_lifecycle
                ),
                AuditContext::for_process(Some(&spawned.session_id), spawned.pid, Some(runtime_id)),
            );
            Ok(spawned)
        }
        Err(err) => {
            if let Err(cleanup_err) = session_registry.delete_session(storage, &session_id) {
                tracing::warn!(
                    session_id,
                    error = %cleanup_err,
                    "PROCESS_RUNTIME: failed to clean up session after spawn failure"
                );
            }
            Err(err)
        }
    }
}

fn validate_backend_class_policy(
    loaded_backend_class: BackendClass,
    required_backend_class: Option<BackendClass>,
) -> Result<(), String> {
    let Some(required_backend_class) = required_backend_class else {
        return Ok(());
    };

    if loaded_backend_class == required_backend_class {
        return Ok(());
    }

    Err(format!(
        "Process routing requires backend class '{}' but the loaded engine is '{}'.",
        required_backend_class.as_str(),
        loaded_backend_class.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::{spawn_managed_process, ManagedProcessRequest};
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
                owner_id: 7,
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
                owner_id: 7,
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
                owner_id: 7,
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
}
