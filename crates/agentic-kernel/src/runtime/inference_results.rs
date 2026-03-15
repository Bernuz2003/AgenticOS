use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::accounting::BackendAccountingEvent;
use crate::audit::{self, AuditContext};
use crate::inference_worker::InferenceResult;
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::{
    kill_managed_process_with_session, release_process_resources_with_session,
};
use crate::session::SessionRegistry;
use crate::storage::{StorageService, StoredAccountingEvent};
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use super::syscalls::{
    dispatch_process_syscall, scan_syscall_buffer, SyscallCmd, SyscallDispatchOutcome,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn drain_worker_results(
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
) {
    while let Ok(result) = result_rx.try_recv() {
        match result {
            InferenceResult::Token {
                pid,
                process,
                text_output,
                generated_tokens,
                finished,
                accounting_event,
            } => {
                let mut process = *process;
                in_flight.remove(&pid);
                scheduler.clear_checked_out_process(pid);
                let Some(runtime_id) = runtime_registry
                    .runtime_id_for_pid(pid)
                    .map(ToString::to_string)
                else {
                    tracing::warn!(
                        pid,
                        "RUNTIME: dropping worker token for unknown runtime pid"
                    );
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
                let pid_floor = runtime_registry.next_pid_floor();
                let audit_context = AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(&runtime_id),
                );
                let (owner_id, syscall_dispatch) = {
                    let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                        tracing::warn!(
                            pid,
                            runtime_id,
                            "RUNTIME: runtime missing engine for worker token"
                        );
                        continue;
                    };

                    if generated_tokens > 0 || !text_output.is_empty() {
                        process.record_model_output(&text_output, generated_tokens);
                    }

                    if !finished
                        && !text_output.is_empty()
                        && crate::prompting::should_stop_on_text_with_metadata(
                            engine.family,
                            &text_output,
                            engine.model_metadata(),
                        )
                    {
                        process.state = if process.lifecycle_policy.is_interactive() {
                            ProcessState::WaitingForInput
                        } else {
                            ProcessState::Finished
                        };
                    }

                    process.mark_resident_prompt_checkpoint();
                    engine.processes.insert(pid, process);

                    if pending_kills.contains(&pid) {
                        pending_kills.retain(|&queued_pid| queued_pid != pid);
                        kill_managed_process_with_session(
                            engine,
                            memory,
                            scheduler,
                            session_registry,
                            storage,
                            pid,
                            "killed",
                        );
                        audit::record(
                            storage,
                            audit::PROCESS_KILLED,
                            "reason=queued_kill_in_flight",
                            audit_context.clone(),
                        );
                        (0, SyscallDispatchOutcome::Killed)
                    } else {
                        let owner_id = engine.process_owner_id(pid).unwrap_or(0);
                        let mut pending_syscall: Option<String> = None;
                        if !text_output.is_empty() {
                            if let Some(proc) = engine.processes.get_mut(&pid) {
                                proc.syscall_buffer.push_str(&text_output);
                                pending_syscall = scan_syscall_buffer(&mut proc.syscall_buffer);
                            }
                        }

                        let syscall_dispatch = if let Some(full_command) = pending_syscall {
                            let content = full_command.trim().to_string();
                            tracing::info!(pid, owner_id, command = %full_command, "OS: SysCall intercepted");
                            dispatch_process_syscall(
                                &runtime_id,
                                pid_floor,
                                engine,
                                memory,
                                scheduler,
                                pid,
                                &content,
                                syscall_cmd_tx,
                                session_registry,
                                storage,
                                pending_events,
                                tool_registry,
                            )
                        } else {
                            SyscallDispatchOutcome::None
                        };
                        (owner_id, syscall_dispatch)
                    }
                };

                if matches!(syscall_dispatch, SyscallDispatchOutcome::Killed) {
                    if let Err(err) = runtime_registry.release_pid(storage, pid) {
                        tracing::warn!(pid, %err, "RUNTIME: failed to release pid after queued kill");
                    }
                    continue;
                }

                let token_quota_exceeded =
                    (0..generated_tokens).any(|_| scheduler.record_token(pid));

                if !text_output.is_empty() && orchestrator.is_orchestrated(pid) {
                    orchestrator.append_output(pid, &text_output);
                }

                if let SyscallDispatchOutcome::Spawned(spawned_pid) = syscall_dispatch {
                    if let Err(err) =
                        runtime_registry.register_pid(storage, &runtime_id, spawned_pid)
                    {
                        tracing::warn!(
                            pid = spawned_pid,
                            runtime_id,
                            %err,
                            "RUNTIME: failed to register syscall-spawned pid"
                        );
                    }
                }

                if !text_output.is_empty() && owner_id > 0 {
                    let token = Token(owner_id);
                    if let Some(client) = clients.get_mut(&token) {
                        client
                            .output_buffer
                            .extend(protocol::response_data(text_output.as_bytes()));
                        let _ = poll.registry().reregister(
                            &mut client.stream,
                            token,
                            Interest::READABLE | Interest::WRITABLE,
                        );
                    }
                }

                if !text_output.is_empty() {
                    pending_events.push(KernelEvent::TimelineChunk {
                        pid,
                        text: text_output.clone(),
                    });
                    pending_events.push(KernelEvent::WorkspaceChanged {
                        pid,
                        reason: "model_output".to_string(),
                    });
                }

                if token_quota_exceeded {
                    tracing::warn!(pid, "SCHEDULER: token quota exceeded — terminating process");
                    if let Some(engine) = runtime_registry.engine_mut(&runtime_id) {
                        if let Some(proc) = engine.processes.get_mut(&pid) {
                            proc.state = ProcessState::Finished;
                        }
                    }
                }

                let turn_state = runtime_registry
                    .engine(&runtime_id)
                    .and_then(|engine| engine.processes.get(&pid))
                    .map(|proc| proc.state.clone());
                if matches!(
                    turn_state,
                    Some(ProcessState::WaitingForInput | ProcessState::AwaitingTurnDecision)
                ) {
                    let sched = scheduler.snapshot(pid);
                    let reason = if matches!(turn_state, Some(ProcessState::AwaitingTurnDecision)) {
                        "awaiting_turn_decision"
                    } else {
                        "turn_completed"
                    };
                    pending_events.push(KernelEvent::SessionFinished {
                        pid,
                        tokens_generated: sched.as_ref().map(|s| s.tokens_generated as u64),
                        elapsed_secs: sched.as_ref().map(|s| s.elapsed_secs),
                        reason: reason.to_string(),
                    });
                    pending_events.push(KernelEvent::WorkspaceChanged {
                        pid,
                        reason: reason.to_string(),
                    });
                    pending_events.push(KernelEvent::LobbyChanged {
                        reason: reason.to_string(),
                    });
                    audit::record(
                        storage,
                        audit::PROCESS_TURN_COMPLETED,
                        format!(
                            "state={:?} tokens={} elapsed={:.3}s reason={}",
                            turn_state,
                            sched.as_ref().map(|s| s.tokens_generated).unwrap_or(0),
                            sched.as_ref().map(|s| s.elapsed_secs).unwrap_or(0.0),
                            reason
                        ),
                        audit_context,
                    );
                }
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
                if orchestrator.is_orchestrated(pid) {
                    orchestrator.mark_failed(pid, &error);
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
}

fn persist_accounting_event(
    storage: &mut StorageService,
    session_registry: &SessionRegistry,
    runtime_registry: &RuntimeRegistry,
    pid: u64,
    runtime_id: &str,
    accounting_event: Option<BackendAccountingEvent>,
) {
    let Some(event) = accounting_event else {
        return;
    };

    let descriptor = runtime_registry.descriptor(runtime_id);
    let session_id = session_registry
        .session_id_for_pid(pid)
        .map(ToString::to_string);
    let record = StoredAccountingEvent {
        session_id,
        pid: Some(pid),
        runtime_id: Some(runtime_id.to_string()),
        backend_id: descriptor
            .map(|runtime| runtime.backend_id.clone())
            .unwrap_or_else(|| event.backend_id.clone()),
        backend_class: descriptor
            .map(|runtime| runtime.backend_class.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        provider_id: descriptor.and_then(|runtime| runtime.provider_id.clone()),
        model_id: descriptor
            .and_then(|runtime| runtime.remote_model_id.clone())
            .or(event.model_id.clone())
            .or_else(|| descriptor.map(|runtime| runtime.logical_model_id.clone())),
        request_kind: "inference_step".to_string(),
        status: event.status,
        request_count: event.request_count,
        stream: event.stream,
        input_tokens: event.input_tokens,
        output_tokens: event.output_tokens,
        estimated_cost_usd: event.estimated_cost_usd,
        error_code: event.error_code,
        error_message: event.error_message,
    };

    if let Err(err) = storage.record_accounting_event(&record) {
        tracing::warn!(
            pid,
            runtime_id,
            %err,
            "ACCOUNTING: failed to persist request accounting event"
        );
        return;
    }

    let audit_context =
        AuditContext::for_process(record.session_id.as_deref(), pid, Some(runtime_id));
    audit::record(
        storage,
        audit::ACCOUNTING_USAGE_RECORDED,
        format!(
            "request_kind={} status={} model={} tokens={}/{} cost=${:.6}",
            record.request_kind,
            record.status.as_str(),
            record.model_id.as_deref().unwrap_or("unknown"),
            record.input_tokens,
            record.output_tokens,
            record.estimated_cost_usd
        ),
        audit_context.clone(),
    );
    if record.estimated_cost_usd > 0.0 {
        audit::record(
            storage,
            audit::ACCOUNTING_COST_RECORDED,
            format!(
                "model={} cost=${:.6} backend={}",
                record.model_id.as_deref().unwrap_or("unknown"),
                record.estimated_cost_usd,
                record.backend_id
            ),
            audit_context,
        );
    }
}
