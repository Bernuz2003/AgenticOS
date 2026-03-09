use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use crate::engine::LLMEngine;
use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{kill_managed_process, spawn_managed_process};
use crate::transport::Client;

pub(super) fn handle_finished_processes(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
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
                        pid,
                        tokens_generated,
                        elapsed_secs,
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
                    client.output_buffer.extend(protocol::response_data(msg.as_bytes()));
                    let _ = poll.registry().reregister(
                        &mut client.stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    );
                }
            }
        }
        kill_managed_process(engine, memory, scheduler, pid);
    }

    for req in spawn_requests {
        match spawn_managed_process(
            engine,
            memory,
            scheduler,
            &req.prompt,
            req.owner_id,
            req.workload,
            ProcessPriority::Normal,
        ) {
            Ok(spawned_process) => {
                let pid = spawned_process.pid;
                orchestrator.register_pid(pid, req.orch_id, &req.task_id);
                tracing::info!(
                    pid,
                    orch_id = req.orch_id,
                    task_id = %req.task_id,
                    "ORCHESTRATOR: spawned dependent task"
                );
            }
            Err(e) => {
                orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, &e.to_string());
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
        if let Some(process) = engine.processes.remove(&pid) {
            if process.state == ProcessState::Finished {
                engine.processes.insert(pid, process);
                continue;
            }
            in_flight.insert(pid);
            let _ = cmd_tx.send(InferenceCmd::Step {
                pid,
                process,
                eos_token_id: eos,
                eot_token_id: eot,
            });
        }
    }
}