pub(crate) mod actions;
mod inference_results;
mod orchestration;
pub(crate) mod syscalls;

use mio::{Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::inference_worker::{InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use inference_results::drain_worker_results;
use orchestration::{advance_orchestrator, checkout_active_processes, handle_finished_processes};
use syscalls::{drain_syscall_results, SyscallCmd, SyscallCompletion};

#[allow(clippy::too_many_arguments)]
pub fn run_engine_tick(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    result_rx: &mpsc::Receiver<InferenceResult>,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    syscall_result_rx: &mpsc::Receiver<SyscallCompletion>,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) {
    let swap_events = memory.poll_swap_events();
    for event in swap_events {
        let Some(engine) = runtime_registry.engine_for_pid_mut(event.pid) else {
            tracing::debug!(
                pid = event.pid,
                slot_id = event.slot_id,
                "MEMORY: dropping swap event for unknown runtime pid"
            );
            continue;
        };

        if event.success {
            match memory.restore_swapped_pid(
                event.pid,
                event.slot_id,
                event.persistence_kind,
                event.swap_path.as_deref(),
            ) {
                Ok(detail) => {
                    if event.persistence_kind.requires_backend_restore() {
                        let Some(path) = event.swap_path.as_deref() else {
                            pending_events.push(KernelEvent::WorkspaceChanged {
                                pid: event.pid,
                                reason: "swap_restore_failed".to_string(),
                            });
                            tracing::error!(
                                pid = event.pid,
                                slot_id = event.slot_id,
                                detail = %event.detail,
                                "MEMORY: backend slot restore missing snapshot path"
                            );
                            continue;
                        };

                        if let Err(err) = engine.mark_process_context_slot_saved(event.pid, path) {
                            pending_events.push(KernelEvent::WorkspaceChanged {
                                pid: event.pid,
                                reason: "swap_restore_failed".to_string(),
                            });
                            tracing::error!(
                                pid = event.pid,
                                slot_id = event.slot_id,
                                persistence_kind = event.persistence_kind.as_str(),
                                detail = %event.detail,
                                %err,
                                "MEMORY: resident slot save bookkeeping failed"
                            );
                            continue;
                        }

                        if let Err(err) = engine.load_process_context_slot(event.pid, path) {
                            pending_events.push(KernelEvent::WorkspaceChanged {
                                pid: event.pid,
                                reason: "swap_restore_failed".to_string(),
                            });
                            tracing::error!(
                                pid = event.pid,
                                slot_id = event.slot_id,
                                persistence_kind = event.persistence_kind.as_str(),
                                detail = %event.detail,
                                %err,
                                "MEMORY: backend slot restore failed"
                            );
                            continue;
                        }
                    }
                    let resumed = engine.set_process_ready_if_parked(event.pid);
                    pending_events.push(KernelEvent::WorkspaceChanged {
                        pid: event.pid,
                        reason: "swap_restored".to_string(),
                    });
                    pending_events.push(KernelEvent::LobbyChanged {
                        reason: "swap_restored".to_string(),
                    });
                    tracing::info!(
                        pid = event.pid,
                        slot_id = event.slot_id,
                        persistence_kind = event.persistence_kind.as_str(),
                        resumed,
                        detail = %detail,
                        "MEMORY: swap complete"
                    );
                }
                Err(err) => {
                    pending_events.push(KernelEvent::WorkspaceChanged {
                        pid: event.pid,
                        reason: "swap_restore_failed".to_string(),
                    });
                    tracing::error!(
                        pid = event.pid,
                        slot_id = event.slot_id,
                        persistence_kind = event.persistence_kind.as_str(),
                        detail = %event.detail,
                        %err,
                        "MEMORY: swap restore failed"
                    );
                }
            }
        } else {
            let resumed = engine.set_process_ready_if_parked(event.pid);
            pending_events.push(KernelEvent::WorkspaceChanged {
                pid: event.pid,
                reason: "swap_failed".to_string(),
            });
            pending_events.push(KernelEvent::LobbyChanged {
                reason: "swap_failed".to_string(),
            });
            tracing::error!(
                pid = event.pid,
                slot_id = event.slot_id,
                persistence_kind = event.persistence_kind.as_str(),
                resumed,
                detail = %event.detail,
                "MEMORY: swap failed"
            );
        }
    }

    drain_syscall_results(
        runtime_registry,
        memory,
        scheduler,
        session_registry,
        storage,
        syscall_result_rx,
        pending_events,
    );

    drain_worker_results(
        runtime_registry,
        memory,
        clients,
        poll,
        scheduler,
        orchestrator,
        result_rx,
        syscall_cmd_tx,
        session_registry,
        storage,
        in_flight,
        pending_kills,
        pending_events,
        tool_registry,
    );

    handle_finished_processes(
        runtime_registry,
        memory,
        clients,
        poll,
        scheduler,
        orchestrator,
        session_registry,
        storage,
        pending_events,
    );
    checkout_active_processes(runtime_registry, scheduler, cmd_tx, in_flight);
    advance_orchestrator(
        runtime_registry,
        resource_governor,
        memory,
        model_catalog,
        clients,
        poll,
        scheduler,
        orchestrator,
        session_registry,
        storage,
        in_flight,
        pending_kills,
        pending_events,
        cmd_tx,
        tool_registry,
    );
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
