use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::process::ProcessLifecyclePolicy;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::orchestration_runtime::resolve_runtime_for_spawn_request;
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_managed_process_with_session, ManagedProcessRequest,
};
use crate::session::SessionRegistry;
use crate::storage::{current_timestamp_ms, StorageService, WorkflowArtifactInputRef};
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::transport::Client;
use crate::{audit, protocol};

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_orchestrator(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    _cmd_tx: &mpsc::Sender<InferenceCmd>,
    tool_registry: &ToolRegistry,
) {
    let (spawn_requests, kill_pids) = orchestrator.advance();
    let system_prompt =
        crate::agent_prompt::build_agent_system_prompt(tool_registry, ToolCaller::AgentSupervisor);

    for pid in kill_pids {
        tracing::warn!(pid, "ORCHESTRATOR: killing task (fail_fast policy)");
        if in_flight.contains(&pid) {
            pending_kills.push(pid);
            continue;
        }
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            continue;
        };
        let owner_id = runtime_registry
            .engine(&runtime_id)
            .and_then(|engine| engine.process_owner_id(pid));
        if let Some(owner_id) = owner_id {
            if owner_id > 0 {
                let token = Token(owner_id);
                if let Some(client) = clients.get_mut(&token) {
                    let msg = format!("\n[ORCHESTRATOR_TASK_KILLED pid={}]\n", pid);
                    client
                        .output_buffer
                        .extend(protocol::response_data(msg.as_bytes()));
                    let _ = poll.registry().reregister(
                        &mut client.stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    );
                }
            }
        }
        pending_events.push(KernelEvent::SessionFinished {
            pid,
            tokens_generated: None,
            elapsed_secs: None,
            reason: "orchestrator_killed".to_string(),
        });
        pending_events.push(KernelEvent::WorkspaceChanged {
            pid,
            reason: "orchestrator_killed".to_string(),
        });
        pending_events.push(KernelEvent::LobbyChanged {
            reason: "orchestrator_killed".to_string(),
        });
        let audit_context = audit::context_for_pid(session_registry, runtime_registry, pid);
        audit::record(
            storage,
            audit::PROCESS_KILLED,
            "reason=orchestrator_killed",
            audit_context,
        );
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
                "orchestrator_killed",
            );
        }
        if let Err(err) = runtime_registry.release_pid(storage, pid) {
            tracing::warn!(pid, %err, "ORCHESTRATOR: failed to release runtime binding on kill");
        }
    }

    for req in spawn_requests {
        let permission_policy = match ProcessPermissionPolicy::workflow_supervisor(
            tool_registry,
            Some(&req.permission_overrides),
        ) {
            Ok(policy) => policy,
            Err(err) => {
                let recorded_at_ms = current_timestamp_ms();
                if let Err(storage_err) = storage.record_workflow_task_spawn_failure(
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                    &err,
                    recorded_at_ms,
                ) {
                    tracing::warn!(
                        orch_id = req.orch_id,
                        task_id = %req.task_id,
                        attempt = req.attempt,
                        %storage_err,
                        "ORCHESTRATOR: failed to persist task permission failure"
                    );
                }
                let _ =
                    orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, req.attempt, &err);
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "orchestrator_spawn_failed".to_string(),
                });
                tracing::error!(task_id = %req.task_id, %err, "ORCHESTRATOR: invalid task permissions");
                continue;
            }
        };
        let runtime_id = match resolve_runtime_for_spawn_request(
            runtime_registry,
            resource_governor,
            storage,
            model_catalog,
            session_registry,
            &req,
        ) {
            Ok(runtime_id) => runtime_id,
            Err(err) => {
                let error = err.to_string();
                if let Err(storage_err) = storage.record_workflow_task_spawn_failure(
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                    &error,
                    current_timestamp_ms(),
                ) {
                    tracing::warn!(
                        orch_id = req.orch_id,
                        task_id = %req.task_id,
                        attempt = req.attempt,
                        %storage_err,
                        "ORCHESTRATOR: failed to persist routing failure"
                    );
                }
                let _ =
                    orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, req.attempt, &error);
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "orchestrator_spawn_failed".to_string(),
                });
                tracing::error!(task_id = %req.task_id, %err, "ORCHESTRATOR: routing failed");
                continue;
            }
        };

        let pid_floor = runtime_registry.next_pid_floor();
        let spawn_result = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                let error = "resolved runtime has no loaded engine";
                if let Err(storage_err) = storage.record_workflow_task_spawn_failure(
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                    error,
                    current_timestamp_ms(),
                ) {
                    tracing::warn!(
                        orch_id = req.orch_id,
                        task_id = %req.task_id,
                        attempt = req.attempt,
                        %storage_err,
                        "ORCHESTRATOR: failed to persist missing-engine failure"
                    );
                }
                let _ =
                    orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, req.attempt, error);
                continue;
            };
            let effective_context_policy = req
                .context_policy
                .align_to_runtime_window_if_default(engine.effective_context_window_tokens());
            spawn_managed_process_with_session(
                &runtime_id,
                pid_floor,
                engine,
                memory,
                scheduler,
                session_registry,
                storage,
                ManagedProcessRequest {
                    prompt: req.prompt.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    owner_id: req.owner_id,
                    tool_caller: ToolCaller::AgentSupervisor,
                    permission_policy: Some(permission_policy),
                    workload: req.workload,
                    required_backend_class: req.required_backend_class,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                    context_policy: Some(effective_context_policy),
                },
            )
        };

        match spawn_result {
            Ok(spawned_process) => {
                let pid = spawned_process.pid;
                if let Err(err) = runtime_registry.register_pid(storage, &runtime_id, pid) {
                    tracing::warn!(
                        pid,
                        runtime_id,
                        %err,
                        "ORCHESTRATOR: failed to register spawned pid"
                    );
                }
                if let Err(err) = storage.begin_workflow_task_attempt(
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                    Some(&spawned_process.session_id),
                    Some(pid),
                    current_timestamp_ms(),
                    &req.input_artifacts
                        .iter()
                        .map(|artifact| WorkflowArtifactInputRef {
                            artifact_id: artifact.artifact_id.clone(),
                            producer_task_id: artifact.producer_task_id.clone(),
                            producer_attempt: artifact.producer_attempt,
                        })
                        .collect::<Vec<_>>(),
                ) {
                    tracing::warn!(
                        orch_id = req.orch_id,
                        task_id = %req.task_id,
                        attempt = req.attempt,
                        %err,
                        "ORCHESTRATOR: failed to persist started task attempt"
                    );
                }
                orchestrator.register_pid(pid, req.orch_id, &req.task_id, req.attempt);
                pending_events.push(KernelEvent::SessionStarted {
                    session_id: spawned_process.session_id.clone(),
                    pid,
                    workload: format!("{:?}", req.workload).to_lowercase(),
                    prompt: req.prompt.clone(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "orchestrator_spawned".to_string(),
                });
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "orchestrator_spawned".to_string(),
                });
                tracing::info!(
                    pid,
                    orch_id = req.orch_id,
                    task_id = %req.task_id,
                    "ORCHESTRATOR: spawned dependent task"
                );
            }
            Err(e) => {
                let error = e.to_string();
                if let Err(storage_err) = storage.record_workflow_task_spawn_failure(
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                    &error,
                    current_timestamp_ms(),
                ) {
                    tracing::warn!(
                        orch_id = req.orch_id,
                        task_id = %req.task_id,
                        attempt = req.attempt,
                        %storage_err,
                        "ORCHESTRATOR: failed to persist spawn failure"
                    );
                }
                let _ =
                    orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, req.attempt, &error);
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "orchestrator_spawn_failed".to_string(),
                });
                tracing::error!(task_id = %req.task_id, %e, "ORCHESTRATOR: spawn failed");
            }
        }
    }
}
