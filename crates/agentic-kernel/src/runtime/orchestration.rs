use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::audit;
use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::orchestration_runtime::resolve_runtime_for_spawn_request;
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_managed_process_with_session, ManagedProcessRequest,
};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::ToolCaller;
use crate::transport::Client;
use crate::{protocol, scheduler::CheckedOutProcessMetadata};

pub(super) fn handle_finished_processes(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
) {
    let finished_pids = runtime_registry.finishable_pids();
    for pid in finished_pids {
        if orchestrator.is_orchestrated(pid) {
            orchestrator.mark_completed(pid);
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
            reason: "completed".to_string(),
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
                "reason=completed tokens={} elapsed={:.3}s",
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
            tracing::warn!(pid, %err, "ORCHESTRATOR: failed to release runtime binding on finish");
        }
    }
}

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
        crate::agent_prompt::build_agent_system_prompt(tool_registry, ToolCaller::AgentText);

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
                orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, &err.to_string());
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
                orchestrator.mark_spawn_failed(
                    req.orch_id,
                    &req.task_id,
                    "resolved runtime has no loaded engine",
                );
                continue;
            };
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
                    workload: req.workload,
                    required_backend_class: req.required_backend_class,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                    context_policy: Some(req.context_policy.clone()),
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
                orchestrator.register_pid(pid, req.orch_id, &req.task_id);
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
                orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, &e.to_string());
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "orchestrator_spawn_failed".to_string(),
                });
                tracing::error!(task_id = %req.task_id, %e, "ORCHESTRATOR: spawn failed");
            }
        }
    }
}

pub(super) fn checkout_active_processes(
    runtime_registry: &mut RuntimeRegistry,
    scheduler: &mut ProcessScheduler,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    in_flight: &mut HashSet<u64>,
) {
    let active_pids = runtime_registry.all_active_pids();
    let ordered_pids = scheduler.scheduling_order(&active_pids);

    for pid in ordered_pids {
        if in_flight.contains(&pid) {
            continue;
        }
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            continue;
        };
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            continue;
        };
        let eos = engine.eos_token_id;
        let eot = engine.eot_token_id;
        if let Some(mut process) = engine.processes.remove(&pid) {
            if !matches!(process.state, ProcessState::Ready | ProcessState::Running) {
                engine.processes.insert(pid, process);
                continue;
            }
            if let Some(event) = process.enforce_context_budget() {
                tracing::info!(
                    pid,
                    strategy = event.strategy.label(),
                    dropped_segments = event.dropped_segments,
                    dropped_tokens = event.dropped_tokens,
                    tokens_after = event.tokens_after,
                    reason = %event.reason,
                    "CONTEXT: pre-step compaction applied"
                );
            }
            scheduler.record_checked_out_process(
                pid,
                CheckedOutProcessMetadata {
                    owner_id: process.owner_id,
                    state: "InFlight".to_string(),
                    tokens: process.tokens.len(),
                    index_pos: process.index_pos,
                    max_tokens: process.max_tokens,
                    context_slot_id: process.context_slot_id,
                    resident_slot_policy: process.resident_slot_policy_label(),
                    resident_slot_state: process.resident_slot_state_label(),
                    resident_slot_snapshot_path: process
                        .resident_slot_snapshot_path()
                        .map(|path| path.display().to_string()),
                    backend_id: Some(process.model.backend_id().to_string()),
                    backend_class: Some(process.model.backend_class().as_str().to_string()),
                    backend_capabilities: Some(process.model.backend_capabilities()),
                    context: process.context_status_snapshot(),
                },
            );
            in_flight.insert(pid);
            let _ = cmd_tx.send(InferenceCmd::Step {
                pid,
                process: Box::new(process),
                eos_token_id: eos,
                eot_token_id: eot,
            });
        }
    }
}
