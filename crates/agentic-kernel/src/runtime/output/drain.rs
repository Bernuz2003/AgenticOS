use mio::{Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::diagnostics::audit;
use crate::inference_worker::InferenceResult;
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::release_process_resources_with_session;
use crate::session::SessionRegistry;
use crate::storage::{current_timestamp_ms, StorageService};
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use crate::runtime::syscalls::SyscallCmd;

use super::stream_path::handle_stream_chunk;
use super::token_path::{handle_token_result, persist_accounting_event};

#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_worker_results(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    result_rx: &mpsc::Receiver<InferenceResult>,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> usize {
    let mut processed_results = 0usize;
    while let Ok(result) = result_rx.try_recv() {
        processed_results = processed_results.saturating_add(1);
        match result {
            InferenceResult::StreamChunk {
                pid,
                text,
                first_chunk,
            } => handle_stream_chunk(
                runtime_registry,
                scheduler,
                clients,
                poll,
                orchestrator,
                pid,
                &text,
                first_chunk,
                session_registry,
                storage,
                pending_events,
            ),
            InferenceResult::Token {
                pid,
                process,
                text_output,
                generated_tokens,
                finished,
                finish_reason,
                accounting_event,
            } => {
                let checked_out = scheduler.take_checked_out_process(pid);
                handle_token_result(
                    runtime_registry,
                    memory,
                    clients,
                    poll,
                    scheduler,
                    orchestrator,
                    pid,
                    *process,
                    text_output,
                    generated_tokens,
                    finished,
                    finish_reason,
                    accounting_event,
                    syscall_cmd_tx,
                    session_registry,
                    storage,
                    checked_out,
                    in_flight,
                    pending_kills,
                    pending_events,
                    tool_registry,
                )
            }
            InferenceResult::Error {
                pid,
                error,
                accounting_event,
            } => {
                in_flight.remove(&pid);
                scheduler.clear_checked_out_process(pid);
                let Some(runtime_id) = runtime_registry
                    .runtime_id_for_pid(pid)
                    .map(ToString::to_string)
                else {
                    tracing::warn!(pid, %error, "RUNTIME: dropping worker error for unknown runtime pid");
                    continue;
                };
                persist_accounting_event(
                    storage,
                    session_registry,
                    runtime_registry,
                    pid,
                    &runtime_id,
                    accounting_event,
                );
                tracing::error!(pid, %error, "Process error from worker, killing");
                if let Some(finalized) = orchestrator.mark_failed(pid, &error, Some("worker_error"))
                {
                    match storage.finalize_workflow_task_attempt(
                        finalized.orch_id,
                        &finalized.task_id,
                        finalized.attempt,
                        &finalized.status,
                        finalized.error.as_deref(),
                        finalized.termination_reason.as_deref(),
                        &finalized.output_text,
                        finalized.truncated,
                        current_timestamp_ms(),
                    ) {
                        Ok(Some(_artifact)) => {}
                        Ok(None) => {}
                        Err(err) => tracing::warn!(
                            orch_id = finalized.orch_id,
                            task_id = %finalized.task_id,
                            attempt = finalized.attempt,
                            %err,
                            "RUNTIME: failed to persist failed workflow task output"
                        ),
                    }
                }
                let audit_context = audit::context_for_pid(session_registry, runtime_registry, pid);
                {
                    let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                        tracing::warn!(pid, runtime_id, %error, "RUNTIME: runtime missing engine for worker error");
                        continue;
                    };
                    release_process_resources_with_session(
                        engine,
                        memory,
                        scheduler,
                        session_registry,
                        storage,
                        pid,
                        "worker_error",
                    );
                    engine.processes.remove(&pid);
                }
                if let Err(err) = runtime_registry.release_pid(storage, pid) {
                    tracing::warn!(pid, %err, "RUNTIME: failed to release pid after worker error");
                }
                audit::record(storage, audit::PROCESS_ERRORED, &error, audit_context);
                pending_events.push(KernelEvent::SessionErrored {
                    pid,
                    message: error,
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "worker_error".to_string(),
                });
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "worker_error".to_string(),
                });
            }
        }
    }

    processed_results
}

#[cfg(test)]
mod tests {
    use super::drain_worker_results;
    use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
    use crate::config::OpenAIResponsesConfig;
    use crate::inference_worker::InferenceResult;
    use crate::memory::NeuralMemory;
    use crate::model_catalog::{RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
    use crate::orchestrator::Orchestrator;
    use crate::process::{ProcessLifecyclePolicy, ProcessState};
    use crate::prompting::PromptFamily;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::scheduler::{CheckedOutProcessMetadata, ProcessPriority, ProcessScheduler};
    use crate::services::process_runtime::{
        spawn_managed_process_with_session, ManagedProcessRequest,
    };
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use crate::tool_registry::ToolRegistry;
    use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};
    use agentic_control_models::KernelEvent;
    use mio::Poll;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn local_style_tool_token_is_dispatched_without_finishing_turn() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let mut fixture = KernelToolDispatchFixture::new().expect("build fixture");
        let pid = fixture
            .spawn_remote_interactive_process("use a tool")
            .expect("spawn process");

        let mut process = fixture.take_process(pid);
        process.state = ProcessState::WaitingForInput;

        fixture
            .result_tx
            .send(InferenceResult::Token {
                pid,
                process: Box::new(process),
                text_output: r#"TOOL:find_files {"pattern":"*.md"}"#.to_string(),
                generated_tokens: 0,
                finished: true,
                finish_reason: Some(crate::backend::InferenceFinishReason::ModelStop),
                accounting_event: None,
            })
            .expect("send worker token");

        let processed = fixture.drain();
        assert_eq!(processed, 1);

        let state = fixture
            .process_state(pid)
            .expect("process state after dispatch");
        assert_eq!(state, ProcessState::WaitingForSyscall);
        fixture.assert_no_turn_completed(pid);
        fixture.assert_no_timeline_chunk_contains("TOOL:find_files");
        fixture.assert_audit_kind_for_pid(pid, "dispatched");
        fixture.assert_syscall_queued(pid, r#"TOOL:find_files {"pattern":"*.md"}"#);
    }

    #[test]
    fn streaming_tool_token_is_dispatched_without_finishing_turn() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let mut fixture = KernelToolDispatchFixture::new().expect("build fixture");
        let pid = fixture
            .spawn_remote_interactive_process("stream a tool")
            .expect("spawn process");

        fixture
            .result_tx
            .send(InferenceResult::StreamChunk {
                pid,
                text: r#"TOOL:find_files {"pattern":"*.md"}"#.to_string(),
                first_chunk: true,
            })
            .expect("send streaming chunk");
        let processed = fixture.drain();
        assert_eq!(processed, 1);
        fixture.assert_no_turn_completed(pid);
        fixture.assert_no_timeline_chunk_contains("TOOL:find_files");
        assert_eq!(
            fixture
                .scheduler
                .checked_out_process(pid)
                .and_then(|metadata| metadata.pending_stream_syscall.as_deref()),
            Some(r#"TOOL:find_files {"pattern":"*.md"}"#)
        );
        assert!(fixture.syscall_cmd_rx.try_recv().is_err());

        let mut process = fixture.take_process(pid);
        process.state = ProcessState::WaitingForInput;
        fixture
            .result_tx
            .send(InferenceResult::Token {
                pid,
                process: Box::new(process),
                text_output: String::new(),
                generated_tokens: 0,
                finished: true,
                finish_reason: Some(crate::backend::InferenceFinishReason::ModelStop),
                accounting_event: None,
            })
            .expect("send worker token");

        let processed = fixture.drain();
        assert_eq!(processed, 1);
        let state = fixture
            .process_state(pid)
            .expect("process state after dispatch");
        assert_eq!(state, ProcessState::WaitingForSyscall);
        fixture.assert_no_turn_completed(pid);
        fixture.assert_no_timeline_chunk_contains("TOOL:find_files");
        fixture.assert_audit_kind_for_pid(pid, "dispatched");
        fixture.assert_syscall_queued(pid, r#"TOOL:find_files {"pattern":"*.md"}"#);
    }

    #[test]
    fn streaming_split_tool_marker_is_dispatched_without_finishing_turn() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let mut fixture = KernelToolDispatchFixture::new().expect("build fixture");
        let pid = fixture
            .spawn_remote_interactive_process("stream a split tool")
            .expect("spawn process");

        for (index, text) in [
            "Analizzo i file:\n\nTO",
            "OL",
            r#":find_files {"pattern":"*.md"}"#,
        ]
        .into_iter()
        .enumerate()
        {
            fixture
                .result_tx
                .send(InferenceResult::StreamChunk {
                    pid,
                    text: text.to_string(),
                    first_chunk: index == 0,
                })
                .expect("send streaming chunk");
            let processed = fixture.drain();
            assert_eq!(processed, 1);
        }

        assert_eq!(
            fixture
                .scheduler
                .checked_out_process(pid)
                .and_then(|metadata| metadata.pending_stream_syscall.as_deref()),
            Some(r#"TOOL:find_files {"pattern":"*.md"}"#)
        );

        let mut process = fixture.take_process(pid);
        process.state = ProcessState::WaitingForInput;
        fixture
            .result_tx
            .send(InferenceResult::Token {
                pid,
                process: Box::new(process),
                text_output: String::new(),
                generated_tokens: 0,
                finished: true,
                finish_reason: Some(crate::backend::InferenceFinishReason::ModelStop),
                accounting_event: None,
            })
            .expect("send worker token");

        let processed = fixture.drain();
        assert_eq!(processed, 1);
        let state = fixture
            .process_state(pid)
            .expect("process state after dispatch");
        assert_eq!(state, ProcessState::WaitingForSyscall);
        fixture.assert_no_turn_completed(pid);
        fixture.assert_no_timeline_chunk_contains("TOOL:find_files");
        fixture.assert_audit_kind_for_pid(pid, "dispatched");
        fixture.assert_syscall_queued(pid, r#"TOOL:find_files {"pattern":"*.md"}"#);
    }

    struct KernelToolDispatchFixture {
        _db_dir: std::path::PathBuf,
        runtime_id: String,
        runtime_registry: RuntimeRegistry,
        memory: NeuralMemory,
        scheduler: ProcessScheduler,
        orchestrator: Orchestrator,
        session_registry: SessionRegistry,
        storage: StorageService,
        pending_events: Vec<KernelEvent>,
        in_flight: HashSet<u64>,
        pending_kills: Vec<u64>,
        result_tx: mpsc::Sender<InferenceResult>,
        result_rx: mpsc::Receiver<InferenceResult>,
        syscall_cmd_tx: mpsc::Sender<crate::runtime::syscalls::SyscallCmd>,
        syscall_cmd_rx: mpsc::Receiver<crate::runtime::syscalls::SyscallCmd>,
        tool_registry: ToolRegistry,
        poll: Poll,
    }

    impl KernelToolDispatchFixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let db_dir = make_temp_dir("agenticos-kernel-tool-dispatch");
            let db_path = db_dir.join("agenticos.db");
            let mut storage = StorageService::open(&db_path)?;
            let boot = storage.record_kernel_boot("0.5.0-test")?;
            let mut runtime_registry = RuntimeRegistry::load(&mut storage)?;
            let runtime = runtime_registry.activate_target(
                &mut storage,
                &remote_target(),
                RuntimeReservation::default(),
            )?;
            let session_registry = SessionRegistry::load(&mut storage, boot.boot_id)?;
            let memory = NeuralMemory::new()?;
            let scheduler = ProcessScheduler::new();
            let orchestrator = Orchestrator::new();
            let (result_tx, result_rx) = mpsc::channel();
            let (syscall_cmd_tx, syscall_cmd_rx) = mpsc::channel();
            Ok(Self {
                _db_dir: db_dir,
                runtime_id: runtime.runtime_id,
                runtime_registry,
                memory,
                scheduler,
                orchestrator,
                session_registry,
                storage,
                pending_events: Vec::new(),
                in_flight: HashSet::new(),
                pending_kills: Vec::new(),
                result_tx,
                result_rx,
                syscall_cmd_tx,
                syscall_cmd_rx,
                tool_registry: ToolRegistry::with_builtins(),
                poll: Poll::new()?,
            })
        }

        fn spawn_remote_interactive_process(&mut self, prompt: &str) -> Result<u64, String> {
            let runtime_id = self.runtime_id.clone();
            let spawned = {
                let pid_floor = self.runtime_registry.next_pid_floor();
                let engine = self
                    .runtime_registry
                    .engine_mut(&runtime_id)
                    .expect("runtime engine");
                spawn_managed_process_with_session(
                    &runtime_id,
                    pid_floor,
                    engine,
                    &mut self.memory,
                    &mut self.scheduler,
                    &mut self.session_registry,
                    &mut self.storage,
                    ManagedProcessRequest {
                        prompt: prompt.to_string(),
                        system_prompt: None,
                        owner_id: 7,
                        tool_caller: ToolCaller::AgentText,
                        permission_policy: Some(test_permissions()),
                        workload: WorkloadClass::Fast,
                        required_backend_class: None,
                        priority: ProcessPriority::Normal,
                        lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                        context_policy: None,
                    },
                )?
            };
            self.runtime_registry
                .register_pid(&mut self.storage, &runtime_id, spawned.pid)
                .expect("register pid");
            self.record_checked_out(spawned.pid);
            self.in_flight.insert(spawned.pid);
            Ok(spawned.pid)
        }

        fn record_checked_out(&mut self, pid: u64) {
            let engine = self
                .runtime_registry
                .engine(&self.runtime_id)
                .expect("runtime engine");
            let process = engine.processes.get(&pid).expect("process");
            self.scheduler.record_checked_out_process(
                pid,
                CheckedOutProcessMetadata {
                    owner_id: process.owner_id,
                    tool_caller: process.tool_caller.clone(),
                    permission_policy: process.permission_policy.clone(),
                    state: format!("{:?}", process.state),
                    checked_out_at: Instant::now(),
                    tokens: process.tokens.len(),
                    index_pos: process.index_pos,
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
                    context: process.context_status_snapshot(),
                    pending_human_request: process.pending_human_request.clone(),
                    pending_output_buffer: String::new(),
                    captured_assistant_text: String::new(),
                    pending_stream_syscall: None,
                },
            );
        }

        fn take_process(&mut self, pid: u64) -> crate::process::AgentProcess {
            self.runtime_registry
                .engine_mut(&self.runtime_id)
                .expect("runtime engine")
                .processes
                .remove(&pid)
                .expect("process to remove")
        }

        fn process_state(&self, pid: u64) -> Option<ProcessState> {
            self.runtime_registry
                .engine(&self.runtime_id)
                .and_then(|engine| engine.processes.get(&pid))
                .map(|process| process.state.clone())
        }

        fn drain(&mut self) -> usize {
            drain_worker_results(
                &mut self.runtime_registry,
                &mut self.memory,
                &mut HashMap::new(),
                &self.poll,
                &mut self.scheduler,
                &mut self.orchestrator,
                &self.result_rx,
                &self.syscall_cmd_tx,
                &mut self.session_registry,
                &mut self.storage,
                &mut self.in_flight,
                &mut self.pending_kills,
                &mut self.pending_events,
                &self.tool_registry,
            )
        }

        fn assert_no_turn_completed(&self, pid: u64) {
            assert!(
                !self.pending_events.iter().any(|event| matches!(
                    event,
                    KernelEvent::SessionFinished { pid: event_pid, .. } if *event_pid == pid
                )),
                "unexpected SessionFinished event: {:?}",
                self.pending_events
            );
        }

        fn assert_no_timeline_chunk_contains(&self, needle: &str) {
            assert!(
                !self.pending_events.iter().any(|event| match event {
                    KernelEvent::TimelineChunk { text, .. } => text.contains(needle),
                    _ => false,
                }),
                "unexpected tool text leaked into timeline: {:?}",
                self.pending_events
            );
        }

        fn assert_syscall_queued(&mut self, pid: u64, expected_command: &str) {
            match self
                .syscall_cmd_rx
                .try_recv()
                .expect("queued syscall command")
            {
                crate::runtime::syscalls::SyscallCmd::Execute {
                    pid: queued_pid,
                    content,
                    ..
                } => {
                    assert_eq!(queued_pid, pid);
                    assert_eq!(content, expected_command);
                }
                other => panic!("unexpected syscall command: {:?}", other),
            }
        }

        fn assert_audit_kind_for_pid(&self, pid: u64, expected_kind: &str) {
            let session_id = self
                .session_registry
                .session_id_for_pid(pid)
                .expect("session id for pid");
            let events = self
                .storage
                .recent_audit_events_for_session(session_id, 64)
                .expect("recent session audit events");
            assert!(
                events
                    .iter()
                    .any(|event| { event.kind == expected_kind && event.pid == Some(pid) }),
                "missing audit kind '{}' for pid {} in {:?}",
                expected_kind,
                pid,
                events
            );
        }
    }

    impl Drop for KernelToolDispatchFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self._db_dir);
        }
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

    fn test_permissions() -> ProcessPermissionPolicy {
        ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: Vec::new(),
            path_scopes: vec![".".to_string()],
        }
    }

    fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
