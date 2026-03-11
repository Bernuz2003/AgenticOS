use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::engine::LLMEngine;
use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{
    kill_managed_process, spawn_managed_process, ManagedProcessRequest,
};
use crate::transport::Client;
use crate::{protocol, scheduler::CheckedOutProcessMetadata};

pub(super) fn handle_finished_processes(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
) {
    let finished_pids = engine.list_finished_pids();
    for pid in finished_pids {
        if orchestrator.is_orchestrated(pid) {
            orchestrator.mark_completed(pid);
        }

        if let Some(owner_id) = engine.process_owner_id(pid) {
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

        kill_managed_process(engine, memory, scheduler, pid);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_orchestrator(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    _cmd_tx: &mpsc::Sender<InferenceCmd>,
) {
    let (spawn_requests, kill_pids) = orchestrator.advance();

    for pid in kill_pids {
        tracing::warn!(pid, "ORCHESTRATOR: killing task (fail_fast policy)");
        if in_flight.contains(&pid) {
            pending_kills.push(pid);
            continue;
        }
        if let Some(owner_id) = engine.process_owner_id(pid) {
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
        kill_managed_process(engine, memory, scheduler, pid);
    }

    for req in spawn_requests {
        match spawn_managed_process(
            engine,
            memory,
            scheduler,
            ManagedProcessRequest {
                prompt: req.prompt.clone(),
                owner_id: req.owner_id,
                workload: req.workload,
                priority: ProcessPriority::Normal,
                context_policy: Some(req.context_policy.clone()),
            },
        ) {
            Ok(spawned_process) => {
                let pid = spawned_process.pid;
                orchestrator.register_pid(pid, req.orch_id, &req.task_id);
                pending_events.push(KernelEvent::SessionStarted {
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
    engine: &mut LLMEngine,
    scheduler: &mut ProcessScheduler,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    in_flight: &mut HashSet<u64>,
) {
    let active_pids = engine.list_active_pids();
    let ordered_pids = scheduler.scheduling_order(&active_pids);
    let eos = engine.eos_token_id;
    let eot = engine.eot_token_id;

    for pid in ordered_pids {
        if in_flight.contains(&pid) {
            continue;
        }
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
