mod input;
pub(crate) mod lifecycle;
mod resume;
mod targeting;

use crate::diagnostics::audit::{self, AuditContext};
use crate::protocol;
use crate::services::process_control::{
    request_process_kill_with_session, request_process_termination_with_session,
    ProcessSignalResult,
};
use agentic_control_models::{KernelEvent, TurnControlResult};
use agentic_protocol::ControlErrorCode;

use self::input::PidPayload;
use super::context::ProcessCommandContext;
use super::diagnostics::log_event;

pub(crate) use input::{handle_continue_output, handle_send_input};
pub(crate) use resume::handle_resume_session;

pub(crate) fn handle_term(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::MissingPid,
            protocol::schema::ERROR,
            "TERM requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let audit_context = AuditContext::for_process(
            ctx.session_registry.session_id_for_pid(pid),
            pid,
            ctx.runtime_registry
                .runtime_id_for_pid(pid)
                .or_else(|| ctx.session_registry.runtime_id_for_pid(pid)),
        );
        match request_process_termination_with_session(
            ctx.runtime_registry,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            ctx.in_flight,
            ctx.pending_kills,
            pid,
        ) {
            ProcessSignalResult::Deferred => {
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "term_queued".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_term",
                    ctx.client_id,
                    Some(pid),
                    "deferred_term_in_flight",
                );
                let message = format!("Termination queued for in-flight PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "TERM",
                    protocol::schema::TERM,
                    &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::Applied => {
                ctx.pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "terminated".to_string(),
                });
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "terminated".to_string(),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "terminated".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_term",
                    ctx.client_id,
                    Some(pid),
                    "graceful_termination_requested",
                );
                audit::record(
                    ctx.storage,
                    audit::PROCESS_TERMINATED,
                    "mode=graceful",
                    audit_context,
                );
                let message = format!("Termination requested for PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "TERM",
                    protocol::schema::TERM,
                    &serde_json::json!({"pid": pid, "status": "requested", "mode": "graceful"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::NotFound => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not found", pid),
            ),
            ProcessSignalResult::NoModelLoaded => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                "No Model Loaded",
            ),
        }
    } else {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidPid,
            protocol::schema::ERROR,
            "TERM payload must be numeric PID",
        )
    }
}

pub(crate) fn handle_kill(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::MissingPid,
            protocol::schema::ERROR,
            "KILL requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let audit_context = AuditContext::for_process(
            ctx.session_registry.session_id_for_pid(pid),
            pid,
            ctx.runtime_registry
                .runtime_id_for_pid(pid)
                .or_else(|| ctx.session_registry.runtime_id_for_pid(pid)),
        );
        match request_process_kill_with_session(
            ctx.runtime_registry,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            ctx.in_flight,
            ctx.pending_kills,
            pid,
        ) {
            ProcessSignalResult::Deferred => {
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "kill_queued".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_kill",
                    ctx.client_id,
                    Some(pid),
                    "deferred_kill_in_flight",
                );
                let message = format!("Kill queued for in-flight PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "KILL",
                    protocol::schema::KILL,
                    &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::Applied => {
                ctx.pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "killed".to_string(),
                });
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "killed".to_string(),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "killed".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_kill",
                    ctx.client_id,
                    Some(pid),
                    "killed_immediately",
                );
                audit::record(
                    ctx.storage,
                    audit::PROCESS_KILLED,
                    "mode=immediate",
                    audit_context,
                );
                let message = format!("Killed PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "KILL",
                    protocol::schema::KILL,
                    &serde_json::json!({"pid": pid, "status": "killed", "mode": "immediate"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::NotFound => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not found", pid),
            ),
            ProcessSignalResult::NoModelLoaded => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                "No Model Loaded",
            ),
        }
    } else {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidPid,
            protocol::schema::ERROR,
            "KILL payload must be numeric PID",
        )
    }
}

pub(crate) fn handle_stop_output(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload = match serde_json::from_slice::<PidPayload>(payload).map_err(|err| err.to_string())
    {
        Ok(value) => value,
        Err(detail) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::StopOutputInvalid,
                protocol::schema::ERROR,
                &format!(
                    "STOP_OUTPUT expects JSON payload {{\"pid\":...}}: {}",
                    detail
                ),
            );
        }
    };

    let Some(runtime_id) = ctx
        .runtime_registry
        .runtime_id_for_pid(payload.pid)
        .or_else(|| ctx.session_registry.runtime_id_for_pid(payload.pid))
        .map(ToString::to_string)
    else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };
    let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let Some(process) = engine.processes.get(&payload.pid) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", payload.pid),
        );
    };

    if process.state != crate::process::ProcessState::AwaitingTurnDecision {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &format!(
                "PID {} is not awaiting a turn decision (state={:?})",
                payload.pid, process.state
            ),
        );
    }

    match engine.stop_current_turn(payload.pid) {
        Ok(()) => {
            if let Some(turn_id) = ctx.session_registry.active_turn_id_for_pid(payload.pid) {
                if let Err(err) =
                    ctx.storage
                        .finish_turn(turn_id, "completed", "output_stopped", None)
                {
                    tracing::warn!(
                        pid = payload.pid,
                        turn_id,
                        %err,
                        "PROCESS_CMD: failed to persist STOP_OUTPUT turn finish"
                    );
                } else {
                    ctx.session_registry.clear_active_turn(payload.pid);
                }
            } else {
                tracing::warn!(
                    pid = payload.pid,
                    "PROCESS_CMD: active turn missing during STOP_OUTPUT"
                );
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: payload.pid,
                reason: "output_stopped".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "output_stopped".to_string(),
            });
            audit::record(
                ctx.storage,
                audit::PROCESS_TURN_COMPLETED,
                "reason=output_stopped",
                AuditContext::for_process(
                    ctx.session_registry.session_id_for_pid(payload.pid),
                    payload.pid,
                    Some(&runtime_id),
                ),
            );
            log_event(
                "process_stop_output",
                ctx.client_id,
                Some(payload.pid),
                "truncated_assistant_turn_confirmed_as_stopped",
            );
            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "STOP_OUTPUT",
                agentic_protocol::schema::STOP_OUTPUT,
                &TurnControlResult {
                    pid: payload.pid,
                    state: "waiting_for_input".to_string(),
                    action: "stop_output".to_string(),
                },
                Some(&format!("Stopped output for PID {}", payload.pid)),
            )
        }
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::handle_send_input;
    use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
    use crate::commands::context::ProcessCommandContext;
    use crate::commands::diagnostics::MetricsState;
    use crate::config::OpenAIResponsesConfig;
    use crate::memory::NeuralMemory;
    use crate::model_catalog::{
        ModelCatalog, RemoteModelEntry, ResolvedModelTarget, WorkloadClass,
    };
    use crate::process::ProcessLifecyclePolicy;
    use crate::prompting::PromptFamily;
    use crate::resource_governor::ResourceGovernor;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::scheduler::{ProcessPriority, ProcessScheduler};
    use crate::services::process_runtime::{
        spawn_managed_process_with_session, ManagedProcessRequest,
    };
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use crate::tool_registry::ToolRegistry;
    use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
    use crate::transport::Client;

    #[test]
    fn send_input_by_session_id_implicitly_resumes_historical_session() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let base = temp_dir("agenticos_process_cmd_resume");
        let db_path = base.join("agenticos.db");

        let session_id = {
            let mut storage = StorageService::open(&db_path).expect("open storage");
            let boot = storage
                .record_kernel_boot("0.5.0-test")
                .expect("record first boot");
            let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load runtimes");
            let mut session_registry =
                SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
            let mut memory = NeuralMemory::new().expect("memory init");
            let mut scheduler = ProcessScheduler::new();
            let tool_registry = ToolRegistry::with_builtins();

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
                    .expect("runtime engine");
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
                                .expect("interactive permissions"),
                        ),
                        workload: WorkloadClass::General,
                        required_backend_class: None,
                        priority: ProcessPriority::Normal,
                        lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                        context_policy: None,
                    },
                )
                .expect("spawn initial session")
            };
            runtime_registry
                .register_pid(&mut storage, &runtime.runtime_id, spawned.pid)
                .expect("register initial pid");

            let turn_id = storage
                .start_session_turn(
                    &spawned.session_id,
                    spawned.pid,
                    "general",
                    "test",
                    "Prima domanda",
                    "prompt",
                )
                .expect("start first turn");
            session_registry.remember_active_turn(spawned.pid, turn_id);
            storage
                .append_assistant_message(turn_id, "Prima risposta")
                .expect("persist assistant message");
            storage
                .finish_turn(turn_id, "completed", "turn_completed", None)
                .expect("finish first turn");
            session_registry.clear_active_turn(spawned.pid);
            session_registry
                .release_pid(&mut storage, spawned.pid, "completed")
                .expect("release session pid");

            spawned.session_id
        };

        let mut storage = StorageService::open(&db_path).expect("reopen storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record second boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("reload runtimes");
        let mut session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("reload sessions");
        let mut model_catalog =
            ModelCatalog::discover(repository_root().join("models")).expect("discover catalog");
        let mut resource_governor =
            ResourceGovernor::load(&mut storage, Default::default()).expect("load governor");
        let mut memory = NeuralMemory::new().expect("memory init");
        let mut scheduler = ProcessScheduler::new();
        let in_flight = HashSet::new();
        let mut pending_kills = Vec::new();
        let mut pending_events = Vec::new();
        let mut metrics = MetricsState::new();
        let tool_registry = ToolRegistry::with_builtins();
        let mut client = test_client();

        let payload = serde_json::to_vec(&json!({
            "session_id": session_id,
            "prompt": "Seconda domanda"
        }))
        .expect("serialize payload");

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
                tool_registry: &tool_registry,
            },
            &payload,
        );

        assert!(
            response.starts_with(b"+OK"),
            "expected +OK response, got: {}",
            String::from_utf8_lossy(&response)
        );

        let resumed_pid = session_registry
            .active_pid_for_session(&session_id)
            .expect("session bound to resumed pid");
        let runtime_id = session_registry
            .runtime_id_for_session(&session_id)
            .expect("runtime id for resumed session")
            .to_string();
        let process = runtime_registry
            .engine(&runtime_id)
            .and_then(|engine| engine.processes.get(&resumed_pid))
            .expect("resumed process");

        assert!(process.prompt_text().contains("Prima domanda"));
        assert!(process.prompt_text().contains("Prima risposta"));
        assert!(process.prompt_text().contains("Seconda domanda"));

        let replay_messages = storage
            .load_replay_messages_for_session(&session_id)
            .expect("load replay messages");
        assert!(replay_messages
            .iter()
            .any(|message| message.content == "Seconda domanda"));
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

    fn test_client() -> Client {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let join = std::thread::spawn(move || listener.accept().expect("accept client").0);
        let client_stream = std::net::TcpStream::connect(addr).expect("connect listener");
        let _server_stream = join.join().expect("join accept thread");
        Client::new(mio::net::TcpStream::from_std(client_stream), true)
    }

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonical repository root")
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
