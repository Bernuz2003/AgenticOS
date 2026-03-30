use mio::{Interest, Poll, Token};
use std::collections::HashMap;

use agentic_control_models::KernelEvent;

use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::{current_timestamp_ms, StorageService, StoredWorkflowArtifact};
use crate::transport::Client;
use crate::{diagnostics::audit, protocol};

use super::lifecycle::termination_reason_for_pid;

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_finished_processes(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
) -> usize {
    let finished_pids = runtime_registry.finishable_pids();
    let mut finished_count = 0usize;
    for pid in finished_pids {
        finished_count = finished_count.saturating_add(1);
        let termination_reason = termination_reason_for_pid(runtime_registry, pid);
        if let Some(finalized) = orchestrator.mark_completed(pid, Some(&termination_reason)) {
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
                Ok(Some(artifact)) => orchestrator.record_completed_artifact(
                    finalized.orch_id,
                    &finalized.task_id,
                    map_stored_artifact(artifact),
                ),
                Ok(None) => {}
                Err(err) => tracing::warn!(
                    orch_id = finalized.orch_id,
                    task_id = %finalized.task_id,
                    attempt = finalized.attempt,
                    %err,
                    "PROCESS_FINISH: failed to persist completed task output"
                ),
            }
        }

        let owner_id = runtime_registry
            .runtime_id_for_pid(pid)
            .and_then(|runtime_id| runtime_registry.engine(runtime_id))
            .and_then(|engine| engine.process_owner_id(pid));

        if let Some(owner_id) = owner_id {
            if owner_id > 0 {
                let token = Token(owner_id);
                if let Some(client) = clients.get_mut(&token) {
                    let sched = scheduler.snapshot(pid);
                    let tokens_generated = sched.as_ref().map(|s| s.tokens_generated).unwrap_or(0);
                    let elapsed_secs = sched.as_ref().map(|s| s.elapsed_secs).unwrap_or(0.0);
                    let end_msg = format!(
                        "\n[PROCESS_FINISHED pid={} tokens_generated={} elapsed_secs={:.3}]\n",
                        pid, tokens_generated, elapsed_secs,
                    );
                    client
                        .output_buffer
                        .extend(protocol::response_data(end_msg.as_bytes()));
                    let _ = poll.registry().reregister(
                        &mut client.stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    );
                }
            }
        }

        let sched = scheduler.snapshot(pid);
        pending_events.push(KernelEvent::SessionFinished {
            pid,
            tokens_generated: sched.as_ref().map(|s| s.tokens_generated as u64),
            elapsed_secs: sched.as_ref().map(|s| s.elapsed_secs),
            reason: termination_reason.clone(),
        });
        pending_events.push(KernelEvent::WorkspaceChanged {
            pid,
            reason: "finished".to_string(),
        });
        pending_events.push(KernelEvent::LobbyChanged {
            reason: "finished".to_string(),
        });
        let audit_context = audit::context_for_pid(session_registry, runtime_registry, pid);
        audit::record(
            storage,
            audit::PROCESS_FINISHED,
            format!(
                "reason={} tokens={} elapsed={:.3}s",
                termination_reason,
                sched
                    .as_ref()
                    .map(|snapshot| snapshot.tokens_generated)
                    .unwrap_or(0),
                sched
                    .as_ref()
                    .map(|snapshot| snapshot.elapsed_secs)
                    .unwrap_or(0.0)
            ),
            audit_context,
        );

        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            continue;
        };
        {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                continue;
            };
            kill_managed_process_with_session(
                engine,
                memory,
                scheduler,
                session_registry,
                storage,
                pid,
                "completed",
            );
        }
        if let Err(err) = runtime_registry.release_pid(storage, pid) {
            tracing::warn!(pid, %err, "PROCESS_FINISH: failed to release runtime binding on finish");
        }
    }

    finished_count
}

fn map_stored_artifact(artifact: StoredWorkflowArtifact) -> crate::orchestrator::TaskArtifact {
    crate::orchestrator::TaskArtifact {
        artifact_id: artifact.artifact_id,
        producer_task_id: artifact.producer_task_id,
        producer_attempt: artifact.producer_attempt,
        mime_type: artifact.mime_type,
        content_text: artifact.content_text,
    }
}

#[cfg(test)]
mod tests {
    use super::handle_finished_processes;
    use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
    use crate::config::OpenAIResponsesConfig;
    use crate::memory::NeuralMemory;
    use crate::model_catalog::{RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
    use crate::orchestrator::Orchestrator;
    use crate::process::ProcessLifecyclePolicy;
    use crate::prompting::PromptFamily;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::scheduler::{ProcessPriority, ProcessScheduler};
    use crate::services::process_runtime::{
        spawn_managed_process_with_session, ManagedProcessRequest,
    };
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};
    use mio::Poll;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn finished_process_is_audited_once_and_removed_from_finishable_set() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let root = make_temp_dir("agenticos-process-finish");
        let db_path = root.join("agenticos.db");
        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load runtimes");
        let runtime = runtime_registry
            .activate_target(
                &mut storage,
                &remote_target(),
                RuntimeReservation::default(),
            )
            .expect("activate runtime");
        let runtime_id = runtime.runtime_id.clone();
        let mut session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
        let mut memory = NeuralMemory::new().expect("memory init");
        let mut scheduler = ProcessScheduler::new();
        let mut orchestrator = Orchestrator::new();
        let poll = Poll::new().expect("poll");
        let mut clients = HashMap::new();
        let mut pending_events = Vec::new();

        let spawned = {
            let pid_floor = runtime_registry.next_pid_floor();
            let engine = runtime_registry
                .engine_mut(&runtime_id)
                .expect("runtime engine");
            spawn_managed_process_with_session(
                &runtime_id,
                pid_floor,
                engine,
                &mut memory,
                &mut scheduler,
                &mut session_registry,
                &mut storage,
                ManagedProcessRequest {
                    prompt: "finish me".to_string(),
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
            )
            .expect("spawn process")
        };
        runtime_registry
            .register_pid(&mut storage, &runtime_id, spawned.pid)
            .expect("register pid");

        {
            let engine = runtime_registry
                .engine_mut(&runtime_id)
                .expect("runtime engine");
            let process = engine.processes.get_mut(&spawned.pid).expect("process");
            process.state = crate::process::ProcessState::Finished;
            process.termination_reason = Some("completed".to_string());
        }

        let finished = handle_finished_processes(
            &mut runtime_registry,
            &mut memory,
            &mut clients,
            &poll,
            &mut scheduler,
            &mut orchestrator,
            &mut session_registry,
            &mut storage,
            &mut pending_events,
        );
        assert_eq!(finished, 1);
        assert!(runtime_registry.runtime_id_for_pid(spawned.pid).is_none());

        let finished_again = handle_finished_processes(
            &mut runtime_registry,
            &mut memory,
            &mut clients,
            &poll,
            &mut scheduler,
            &mut orchestrator,
            &mut session_registry,
            &mut storage,
            &mut pending_events,
        );
        assert_eq!(finished_again, 0);

        let audit_events = storage
            .recent_audit_events_for_session(&spawned.session_id, 64)
            .expect("audit events");
        let process_finished_events = audit_events
            .iter()
            .filter(|event| event.kind == "finished")
            .count();
        assert_eq!(process_finished_events, 1);

        let _ = fs::remove_dir_all(root);
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
