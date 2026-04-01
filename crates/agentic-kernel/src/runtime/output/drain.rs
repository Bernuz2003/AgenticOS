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
use super::turn_assembly::TurnAssemblyStore;

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
    turn_assembly: &mut TurnAssemblyStore,
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
                turn_assembly,
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
                    turn_assembly,
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
