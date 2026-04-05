use std::collections::HashSet;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Instant;

use agentic_control_models::{
    AssistantSegmentKind, CoreDumpCaptureResult, CoreDumpInfoResponse, CoreDumpListResponse,
    CoreDumpReplayRequest,
};
use serde_json::{json, Value};

use crate::backend::resolve_driver_for_model;
use crate::commands::{
    handle_core_dump, handle_core_dump_info, handle_list_core_dumps, CoreDumpCommandContext,
    MetricsState, ProcessCommandContext,
};
use crate::config::OpenAIResponsesConfig;
use crate::core_dump::{apply_core_dump_retention, replay_core_dump, CoreDumpRetentionPolicy};
use crate::memory::NeuralMemory;
use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::prompting::PromptFamily;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
use crate::scheduler::{CheckedOutProcessMetadata, ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::storage::{current_timestamp_ms, NewCoreDumpRecord};
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::transport::Client;

use super::helpers::{create_temp_dir, remove_temp_dir};

#[derive(Debug, Clone)]
pub struct CoreDumpObservation {
    pub dump_id: String,
    pub session_id: String,
    pub pid: u64,
    pub list_dump_ids: Vec<String>,
    pub manifest: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayCoreDumpObservation {
    pub source_session_id: String,
    pub source_pid: u64,
    pub replay_session_id: String,
    pub replay_pid: u64,
    pub replay_title: String,
    pub replay_tool_mode: String,
    pub original_allowed_tools: Vec<String>,
    pub replay_allowed_tools: Vec<String>,
    pub replay_messages: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDumpRetentionObservation {
    pub pruned_dump_ids: Vec<String>,
    pub remaining_dump_ids: Vec<String>,
    pub stale_index_entries: usize,
}

pub fn manual_core_dump_live_observation() -> Result<CoreDumpObservation, String> {
    run_manual_core_dump_observation(false)
}

pub fn manual_core_dump_checked_out_observation() -> Result<CoreDumpObservation, String> {
    run_manual_core_dump_observation(true)
}

fn run_manual_core_dump_observation(checked_out: bool) -> Result<CoreDumpObservation, String> {
    let root = TempDirGuard::new("agenticos-core-dump")?;
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
    let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
    let mut scheduler = ProcessScheduler::new();
    let tool_registry = ToolRegistry::with_builtins();
    let mut turn_assembly = TurnAssemblyStore::default();
    let mut client = test_client()?;
    let mut in_flight = HashSet::new();
    let mut pending_events = Vec::new();

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
                owner_id: 17,
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
        .register_pid(&mut storage, &runtime_id, spawned.pid)
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
    let _ = turn_assembly.consume_stream_fragment(
        spawned.pid,
        AssistantSegmentKind::Message,
        "Risposta parziale",
    );

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

    if checked_out {
        let engine = runtime_registry
            .engine_mut(&runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?;
        let process = engine
            .processes
            .remove(&spawned.pid)
            .ok_or_else(|| "spawned process missing".to_string())?;
        scheduler.record_checked_out_process(
            spawned.pid,
            CheckedOutProcessMetadata {
                owner_id: process.owner_id,
                tool_caller: process.tool_caller.clone(),
                permission_policy: process.permission_policy.clone(),
                state: checked_out_state_label(engine.loaded_backend_class().as_str()),
                checked_out_at: Instant::now(),
                token_count: process.tokens.len(),
                tokens: process.tokens.clone(),
                index_pos: process.index_pos,
                turn_start_index: process.turn_start_index,
                max_tokens: process.max_tokens,
                context_slot_id: process.context_slot_id,
                resident_slot_policy: process.resident_slot_policy_label(),
                resident_slot_state: process.resident_slot_state_label(),
                resident_slot_snapshot_path: process
                    .resident_slot_snapshot_path()
                    .map(|path| path.display().to_string()),
                backend_id: Some(engine.loaded_backend_id().to_string()),
                backend_class: Some(engine.loaded_backend_class().as_str().to_string()),
                backend_capabilities: Some(engine.loaded_backend_capabilities()),
                prompt_text: process.prompt_text().to_string(),
                resident_prompt_checkpoint_bytes: process.resident_prompt_checkpoint_bytes(),
                context_policy: process.context_policy.clone(),
                context_state: process.context_state.clone(),
                context: process.context_status_snapshot(),
                pending_human_request: process.pending_human_request.clone(),
                termination_reason: process.termination_reason.clone(),
            },
        );
        in_flight.insert(spawned.pid);
    }

    let capture = parse_ok::<CoreDumpCaptureResult>(handle_core_dump(
        CoreDumpCommandContext {
            client: &mut client,
            request_id: "test:coredump",
            runtime_registry: &mut runtime_registry,
            scheduler: &mut scheduler,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            memory: &mut memory,
            in_flight: &in_flight,
            pending_events: &mut pending_events,
        },
        &serde_json::to_vec(&json!({
            "pid": spawned.pid,
            "reason": if checked_out { "checked_out_test" } else { "live_test" },
            "include_backend_state": true
        }))
        .map_err(|err| err.to_string())?,
    ))?;

    let list = parse_ok::<CoreDumpListResponse>(handle_list_core_dumps(
        CoreDumpCommandContext {
            client: &mut client,
            request_id: "test:list",
            runtime_registry: &mut runtime_registry,
            scheduler: &mut scheduler,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            memory: &mut memory,
            in_flight: &in_flight,
            pending_events: &mut pending_events,
        },
        &serde_json::to_vec(&json!({ "limit": 8 })).map_err(|err| err.to_string())?,
    ))?;

    let info = parse_ok::<CoreDumpInfoResponse>(handle_core_dump_info(
        CoreDumpCommandContext {
            client: &mut client,
            request_id: "test:info",
            runtime_registry: &mut runtime_registry,
            scheduler: &mut scheduler,
            session_registry: &mut session_registry,
            storage: &mut storage,
            turn_assembly: &mut turn_assembly,
            memory: &mut memory,
            in_flight: &in_flight,
            pending_events: &mut pending_events,
        },
        &serde_json::to_vec(&json!({ "dump_id": capture.dump.dump_id }))
            .map_err(|err| err.to_string())?,
    ))?;

    let manifest =
        serde_json::from_str::<Value>(&info.manifest_json).map_err(|err| err.to_string())?;
    let artifact_path = PathBuf::from(&capture.dump.path);
    let _ = std::fs::remove_file(&artifact_path);

    Ok(CoreDumpObservation {
        dump_id: capture.dump.dump_id,
        session_id: spawned.session_id,
        pid: spawned.pid,
        list_dump_ids: list.dumps.into_iter().map(|dump| dump.dump_id).collect(),
        manifest,
    })
}

pub fn replay_core_dump_observation() -> Result<ReplayCoreDumpObservation, String> {
    let root = TempDirGuard::new("agenticos-core-dump-replay")?;
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
    let mut memory = NeuralMemory::new().map_err(|err| err.to_string())?;
    let mut scheduler = ProcessScheduler::new();
    let tool_registry = ToolRegistry::with_builtins();
    let mut turn_assembly = TurnAssemblyStore::default();
    let in_flight = HashSet::new();
    let mut pending_events = Vec::new();

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
                owner_id: 17,
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
        .register_pid(&mut storage, &runtime_id, spawned.pid)
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
    let _ = turn_assembly.consume_stream_fragment(
        spawned.pid,
        AssistantSegmentKind::Message,
        "Risposta parziale",
    );

    let dump = crate::core_dump::capture_core_dump(
        crate::core_dump::CaptureCoreDumpArgs {
            runtime_registry: &runtime_registry,
            scheduler: &scheduler,
            session_registry: &session_registry,
            storage: &mut storage,
            turn_assembly: &turn_assembly,
            memory: &memory,
            in_flight: &in_flight,
        },
        agentic_control_models::CoreDumpRequest {
            session_id: Some(spawned.session_id.clone()),
            pid: Some(spawned.pid),
            mode: Some("manual".to_string()),
            reason: Some("replay_branch_test".to_string()),
            include_workspace: Some(false),
            include_backend_state: Some(false),
            freeze_target: Some(false),
            note: None,
        },
    )?;

    let original_allowed_tools = runtime_registry
        .engine(&runtime_id)
        .and_then(|engine| engine.processes.get(&spawned.pid))
        .map(|process| process.permission_policy.allowed_tools.clone())
        .ok_or_else(|| "missing source process".to_string())?;

    let mut model_catalog = ModelCatalog::discover(crate::config::repository_root().join("models"))
        .map_err(|err| err.to_string())?;
    let mut resource_governor =
        ResourceGovernor::load(&mut storage, Default::default()).map_err(|err| err.to_string())?;
    let mut pending_kills = Vec::new();
    let mut metrics = MetricsState::new();
    let mut client = test_client()?;
    let replay = replay_core_dump(
        &mut ProcessCommandContext {
            client: &mut client,
            request_id: "test:replay",
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
        CoreDumpReplayRequest {
            dump_id: dump.dump_id,
            branch_label: Some("Counterfactual branch".to_string()),
            tool_mode: None,
            patch: None,
        },
    )
    .map_err(|(_, message)| message)?;

    let replay_allowed_tools = runtime_registry
        .engine(&replay.runtime_id)
        .and_then(|engine| engine.processes.get(&replay.pid))
        .map(|process| process.permission_policy.allowed_tools.clone())
        .ok_or_else(|| "missing replay process".to_string())?;
    let replay_messages = storage
        .load_replay_messages_for_session(&replay.session_id)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|message| (message.role, message.kind, message.content))
        .collect();

    Ok(ReplayCoreDumpObservation {
        source_session_id: spawned.session_id,
        source_pid: spawned.pid,
        replay_session_id: replay.session_id,
        replay_pid: replay.pid,
        replay_title: replay.replay_session_title,
        replay_tool_mode: replay.tool_mode,
        original_allowed_tools,
        replay_allowed_tools,
        replay_messages,
    })
}

pub fn core_dump_retention_observation() -> Result<CoreDumpRetentionObservation, String> {
    let root = TempDirGuard::new("agenticos-core-dump-retention")?;
    let db_path = root.path().join("agenticos.db");
    let dump_dir = root.path().join("dumps");
    std::fs::create_dir_all(&dump_dir).map_err(|err| err.to_string())?;

    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let now_ms = current_timestamp_ms();
    let records = vec![
        (
            "dump-new",
            now_ms,
            dump_dir.join("dump-new.agentcore.zst"),
            true,
        ),
        (
            "dump-old",
            now_ms - 10_000,
            dump_dir.join("dump-old.agentcore.zst"),
            true,
        ),
        (
            "dump-missing",
            now_ms - 20_000,
            dump_dir.join("dump-missing.agentcore.zst"),
            false,
        ),
    ];

    for (dump_id, created_at_ms, path, exists) in &records {
        if *exists {
            std::fs::write(path, dump_id.as_bytes()).map_err(|err| err.to_string())?;
        }
        storage
            .record_core_dump(&NewCoreDumpRecord {
                dump_id: (*dump_id).to_string(),
                created_at_ms: *created_at_ms,
                session_id: None,
                pid: None,
                reason: "test".to_string(),
                fidelity: "full_context_snapshot".to_string(),
                path: path.display().to_string(),
                bytes: dump_id.len(),
                sha256: dump_id.to_string(),
                note: None,
            })
            .map_err(|err| err.to_string())?;
    }

    let outcome = apply_core_dump_retention(
        &mut storage,
        CoreDumpRetentionPolicy {
            max_files: Some(1),
            max_age_ms: None,
        },
    )?;
    let remaining_dump_ids = storage
        .load_all_core_dump_records()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|record| record.dump_id)
        .collect();

    Ok(CoreDumpRetentionObservation {
        pruned_dump_ids: outcome.pruned_dump_ids,
        remaining_dump_ids,
        stale_index_entries: outcome.stale_index_entries,
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

fn parse_ok<T: serde::de::DeserializeOwned>(response: Vec<u8>) -> Result<T, String> {
    if !response.starts_with(b"+OK ") {
        let payload = extract_payload(&response)?;
        return Err(format!(
            "expected +OK response, got {}",
            String::from_utf8_lossy(&payload)
        ));
    }
    serde_json::from_slice(&extract_payload(&response)?).map_err(|err| err.to_string())
}

fn extract_payload(response: &[u8]) -> Result<Vec<u8>, String> {
    let marker = b"\r\n";
    let Some(header_end) = response
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return Err("response header missing CRLF".to_string());
    };
    Ok(response[header_end + marker.len()..].to_vec())
}

fn checked_out_state_label(backend_class: &str) -> String {
    if backend_class == "remote_stateless" {
        "AwaitingRemoteResponse".to_string()
    } else {
        "InFlight".to_string()
    }
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

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Result<Self, String> {
        create_temp_dir(prefix)
            .map(Self)
            .map_err(|err| err.to_string())
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        remove_temp_dir(&self.0);
    }
}
