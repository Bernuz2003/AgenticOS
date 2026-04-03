use std::collections::HashSet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::backend::resolve_driver_for_model;
use crate::commands::{handle_send_input, handle_stop_output, MetricsState, ProcessCommandContext};
use crate::config::OpenAIResponsesConfig;
use crate::memory::NeuralMemory;
use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::prompting::PromptFamily;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::transport::Client;

use super::helpers::{create_temp_dir, remove_temp_dir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendInputResumeObservation {
    pub response_ok: bool,
    pub resumed_pid: u64,
    pub prompt_text: String,
    pub replay_messages: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopOutputObservation {
    pub response_ok: bool,
    pub active_turn_cleared: bool,
    pub pending_segments_cleared: bool,
    pub replay_messages: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningStopOutputObservation {
    pub response_ok: bool,
    pub active_turn_preserved: bool,
    pub stop_requested: bool,
}

pub fn send_input_by_session_id_resume_observation() -> Result<SendInputResumeObservation, String> {
    let root = TempDirGuard::new("agenticos-process-cmd-resume")?;
    let db_path = root.path().join("agenticos.db");

    let session_id = {
        let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .map_err(|err| err.to_string())?;
        let mut runtime_registry =
            RuntimeRegistry::load(&mut storage).map_err(|err| err.to_string())?;
        let mut session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).map_err(|err| err.to_string())?;
        let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
        let mut scheduler = ProcessScheduler::new();
        let tool_registry = ToolRegistry::with_builtins();

        let runtime = runtime_registry
            .activate_target(
                &mut storage,
                &remote_target(),
                RuntimeReservation::default(),
            )
            .map_err(|err| err.to_string())?;
        let spawned = {
            let pid_floor = runtime_registry.next_pid_floor();
            let engine = runtime_registry
                .engine_mut(&runtime.runtime_id)
                .ok_or_else(|| "runtime engine missing".to_string())?;
            spawn_managed_process_with_session(
                &runtime.runtime_id,
                pid_floor,
                engine,
                &mut memory,
                &mut scheduler,
                &mut session_registry,
                &mut storage,
                ManagedProcessRequest {
                    prompt: "Prima domanda".to_string(),
                    system_prompt: None,
                    owner_id: 41,
                    tool_caller: ToolCaller::AgentText,
                    permission_policy: Some(
                        ProcessPermissionPolicy::interactive_chat(&tool_registry)
                            .map_err(|err| err.to_string())?,
                    ),
                    workload: WorkloadClass::General,
                    required_backend_class: None,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                    context_policy: None,
                    quota_override: None,
                },
            )
            .map_err(|err| err.to_string())?
        };
        runtime_registry
            .register_pid(&mut storage, &runtime.runtime_id, spawned.pid)
            .map_err(|err| err.to_string())?;

        let turn_id = storage
            .start_session_turn(
                &spawned.session_id,
                spawned.pid,
                "general",
                "test",
                "Prima domanda",
                "prompt",
            )
            .map_err(|err| err.to_string())?;
        session_registry.remember_active_turn(spawned.pid, turn_id);
        storage
            .append_assistant_message(turn_id, "Prima risposta")
            .map_err(|err| err.to_string())?;
        storage
            .finish_turn(turn_id, "completed", "turn_completed", None)
            .map_err(|err| err.to_string())?;
        session_registry.clear_active_turn(spawned.pid);
        session_registry
            .release_pid(&mut storage, spawned.pid, "completed")
            .map_err(|err| err.to_string())?;

        spawned.session_id
    };

    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;
    let mut runtime_registry =
        RuntimeRegistry::load(&mut storage).map_err(|err| err.to_string())?;
    let mut session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).map_err(|err| err.to_string())?;
    let mut model_catalog =
        ModelCatalog::discover(repository_root().join("models")).map_err(|err| err.to_string())?;
    let mut resource_governor =
        ResourceGovernor::load(&mut storage, Default::default()).map_err(|err| err.to_string())?;
    let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
    let mut scheduler = ProcessScheduler::new();
    let in_flight = HashSet::new();
    let mut pending_kills = Vec::new();
    let mut pending_events = Vec::new();
    let mut turn_assembly = TurnAssemblyStore::default();
    let mut metrics = MetricsState::new();
    let tool_registry = ToolRegistry::with_builtins();
    let mut client = test_client()?;

    let payload = serde_json::to_vec(&json!({
        "session_id": session_id,
        "prompt": "Seconda domanda"
    }))
    .map_err(|err| err.to_string())?;

    let response = handle_send_input(
        ProcessCommandContext {
            client: &mut client,
            request_id: "test:1",
            runtime_registry: &mut runtime_registry,
            resource_governor: &mut resource_governor,
            model_catalog: &mut model_catalog,
            memory: &mut memory,
            scheduler: &mut scheduler,
            in_flight: &in_flight,
            pending_kills: &mut pending_kills,
            pending_events: &mut pending_events,
            metrics: &mut metrics,
            client_id: 99,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            tool_registry: &tool_registry,
        },
        &payload,
    );

    let resumed_pid = session_registry
        .active_pid_for_session(&session_id)
        .ok_or_else(|| "session not rebound to a live pid".to_string())?;
    let runtime_id = session_registry
        .runtime_id_for_session(&session_id)
        .ok_or_else(|| "missing runtime id for resumed session".to_string())?
        .to_string();
    let prompt_text = runtime_registry
        .engine(&runtime_id)
        .and_then(|engine| engine.processes.get(&resumed_pid))
        .map(|process| process.prompt_text().to_string())
        .ok_or_else(|| "missing resumed process".to_string())?;
    let replay_messages = storage
        .load_replay_messages_for_session(&session_id)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|message| (message.role, message.kind, message.content))
        .collect();

    Ok(SendInputResumeObservation {
        response_ok: response.starts_with(b"+OK"),
        resumed_pid,
        prompt_text,
        replay_messages,
    })
}

pub fn stop_output_flush_observation() -> Result<StopOutputObservation, String> {
    let root = TempDirGuard::new("agenticos-process-cmd-stop-output")?;
    let db_path = root.path().join("agenticos.db");

    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;
    let mut runtime_registry =
        RuntimeRegistry::load(&mut storage).map_err(|err| err.to_string())?;
    let runtime = runtime_registry
        .activate_target(
            &mut storage,
            &remote_target(),
            RuntimeReservation::default(),
        )
        .map_err(|err| err.to_string())?;
    let runtime_id = runtime.runtime_id.clone();
    let mut session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).map_err(|err| err.to_string())?;
    let mut model_catalog =
        ModelCatalog::discover(repository_root().join("models")).map_err(|err| err.to_string())?;
    let mut resource_governor =
        ResourceGovernor::load(&mut storage, Default::default()).map_err(|err| err.to_string())?;
    let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
    let mut scheduler = ProcessScheduler::new();
    let in_flight = HashSet::new();
    let mut pending_kills = Vec::new();
    let mut pending_events = Vec::new();
    let mut turn_assembly = TurnAssemblyStore::default();
    let mut metrics = MetricsState::new();
    let tool_registry = ToolRegistry::with_builtins();
    let mut client = test_client()?;

    let spawned = {
        let pid_floor = runtime_registry.next_pid_floor();
        let engine = runtime_registry
            .engine_mut(&runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?;
        spawn_managed_process_with_session(
            &runtime_id,
            pid_floor,
            engine,
            &mut memory,
            &mut scheduler,
            &mut session_registry,
            &mut storage,
            ManagedProcessRequest {
                prompt: "Prima domanda".to_string(),
                system_prompt: None,
                owner_id: 7,
                tool_caller: ToolCaller::AgentText,
                permission_policy: Some(
                    ProcessPermissionPolicy::interactive_chat(&tool_registry)
                        .map_err(|err| err.to_string())?,
                ),
                workload: WorkloadClass::Fast,
                required_backend_class: None,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
                quota_override: None,
            },
        )
        .map_err(|err| err.to_string())?
    };
    runtime_registry
        .register_pid(&mut storage, &runtime_id, spawned.pid)
        .map_err(|err| err.to_string())?;
    let turn_id = storage
        .start_session_turn(
            &spawned.session_id,
            spawned.pid,
            "fast",
            "test",
            "Prima domanda",
            "prompt",
        )
        .map_err(|err| err.to_string())?;
    session_registry.remember_active_turn(spawned.pid, turn_id);

    {
        let engine = runtime_registry
            .engine_mut(&runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?;
        let process = engine
            .processes
            .get_mut(&spawned.pid)
            .ok_or_else(|| "spawned process missing".to_string())?;
        process.state = ProcessState::AwaitingTurnDecision;
    }

    let finalized = turn_assembly.consume_final_fragments(spawned.pid, "Risposta parziale", "");
    if finalized.complete_assistant_text != "Risposta parziale" {
        return Err("unexpected finalized assistant text".to_string());
    }

    let payload =
        serde_json::to_vec(&json!({ "pid": spawned.pid })).map_err(|err| err.to_string())?;
    let response = handle_stop_output(
        ProcessCommandContext {
            client: &mut client,
            request_id: "test:stop",
            runtime_registry: &mut runtime_registry,
            resource_governor: &mut resource_governor,
            model_catalog: &mut model_catalog,
            memory: &mut memory,
            scheduler: &mut scheduler,
            in_flight: &in_flight,
            pending_kills: &mut pending_kills,
            pending_events: &mut pending_events,
            metrics: &mut metrics,
            client_id: 99,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            tool_registry: &tool_registry,
        },
        &payload,
    );

    let replay_messages = storage
        .load_replay_messages_for_session(&spawned.session_id)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|message| (message.role, message.kind, message.content))
        .collect();

    Ok(StopOutputObservation {
        response_ok: response.starts_with(b"+OK"),
        active_turn_cleared: session_registry
            .active_turn_id_for_pid(spawned.pid)
            .is_none(),
        pending_segments_cleared: turn_assembly.drain_pending_segments(spawned.pid).is_none(),
        replay_messages,
    })
}

pub fn request_stop_output_while_running_observation(
) -> Result<RunningStopOutputObservation, String> {
    let root = TempDirGuard::new("agenticos-process-cmd-soft-stop")?;
    let db_path = root.path().join("agenticos.db");

    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;
    let mut runtime_registry =
        RuntimeRegistry::load(&mut storage).map_err(|err| err.to_string())?;
    let runtime = runtime_registry
        .activate_target(
            &mut storage,
            &remote_target(),
            RuntimeReservation::default(),
        )
        .map_err(|err| err.to_string())?;
    let runtime_id = runtime.runtime_id.clone();
    let mut session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).map_err(|err| err.to_string())?;
    let mut model_catalog =
        ModelCatalog::discover(repository_root().join("models")).map_err(|err| err.to_string())?;
    let mut resource_governor =
        ResourceGovernor::load(&mut storage, Default::default()).map_err(|err| err.to_string())?;
    let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
    let mut scheduler = ProcessScheduler::new();
    let mut in_flight = HashSet::new();
    let mut pending_kills = Vec::new();
    let mut pending_events = Vec::new();
    let mut turn_assembly = TurnAssemblyStore::default();
    let mut metrics = MetricsState::new();
    let tool_registry = ToolRegistry::with_builtins();
    let mut client = test_client()?;

    let spawned = {
        let pid_floor = runtime_registry.next_pid_floor();
        let engine = runtime_registry
            .engine_mut(&runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?;
        spawn_managed_process_with_session(
            &runtime_id,
            pid_floor,
            engine,
            &mut memory,
            &mut scheduler,
            &mut session_registry,
            &mut storage,
            ManagedProcessRequest {
                prompt: "Prima domanda".to_string(),
                system_prompt: None,
                owner_id: 7,
                tool_caller: ToolCaller::AgentText,
                permission_policy: Some(
                    ProcessPermissionPolicy::interactive_chat(&tool_registry)
                        .map_err(|err| err.to_string())?,
                ),
                workload: WorkloadClass::Fast,
                required_backend_class: None,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
                quota_override: None,
            },
        )
        .map_err(|err| err.to_string())?
    };
    runtime_registry
        .register_pid(&mut storage, &runtime_id, spawned.pid)
        .map_err(|err| err.to_string())?;
    let turn_id = storage
        .start_session_turn(
            &spawned.session_id,
            spawned.pid,
            "fast",
            "test",
            "Prima domanda",
            "prompt",
        )
        .map_err(|err| err.to_string())?;
    session_registry.remember_active_turn(spawned.pid, turn_id);
    in_flight.insert(spawned.pid);

    if let Some(engine) = runtime_registry.engine_mut(&runtime_id) {
        let _ = engine.processes.remove(&spawned.pid);
    }

    let payload =
        serde_json::to_vec(&json!({ "pid": spawned.pid })).map_err(|err| err.to_string())?;
    let response = handle_stop_output(
        ProcessCommandContext {
            client: &mut client,
            request_id: "test:soft-stop",
            runtime_registry: &mut runtime_registry,
            resource_governor: &mut resource_governor,
            model_catalog: &mut model_catalog,
            memory: &mut memory,
            scheduler: &mut scheduler,
            in_flight: &in_flight,
            pending_kills: &mut pending_kills,
            pending_events: &mut pending_events,
            metrics: &mut metrics,
            client_id: 99,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            tool_registry: &tool_registry,
        },
        &payload,
    );

    Ok(RunningStopOutputObservation {
        response_ok: response.starts_with(b"+OK"),
        active_turn_preserved: session_registry.active_turn_id_for_pid(spawned.pid)
            == Some(turn_id),
        stop_requested: turn_assembly.output_stop_requested(spawned.pid),
    })
}

fn test_openai_config() -> OpenAIResponsesConfig {
    OpenAIResponsesConfig {
        endpoint: "https://api.openai.example/v1/responses".to_string(),
        api_key: "test-key".to_string(),
        default_model: "gpt-4.1-mini".to_string(),
        stream: true,
        ..Default::default()
    }
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

fn test_client() -> Result<Client, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let addr = listener.local_addr().map_err(|err| err.to_string())?;
    let join = std::thread::spawn(move || listener.accept().map(|pair| pair.0));
    let client_stream = std::net::TcpStream::connect(addr).map_err(|err| err.to_string())?;
    let _server_stream = join
        .join()
        .map_err(|_| "accept thread panicked".to_string())?
        .map_err(|err| err.to_string())?;
    Ok(Client::new(
        mio::net::TcpStream::from_std(client_stream),
        true,
    ))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repository root")
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Result<Self, String> {
        create_temp_dir(prefix)
            .map(Self)
            .map_err(|err| err.to_string())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        remove_temp_dir(&self.0);
    }
}
