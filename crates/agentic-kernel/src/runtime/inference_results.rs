use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::engine::LLMEngine;
use crate::inference_worker::InferenceResult;
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::kill_managed_process;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use super::syscalls::{dispatch_process_syscall, scan_syscall_buffer, SyscallCmd};

#[allow(clippy::too_many_arguments)]
pub(super) fn drain_worker_results(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    result_rx: &mpsc::Receiver<InferenceResult>,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
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
            } => {
                let mut process = *process;
                in_flight.remove(&pid);
                scheduler.clear_checked_out_process(pid);

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
                    kill_managed_process(engine, memory, scheduler, pid);
                    continue;
                }

                let owner_id = engine.process_owner_id(pid).unwrap_or(0);
                let token_quota_exceeded =
                    (0..generated_tokens).any(|_| scheduler.record_token(pid));

                if !text_output.is_empty() && orchestrator.is_orchestrated(pid) {
                    orchestrator.append_output(pid, &text_output);
                }

                let mut pending_syscall: Option<String> = None;
                if !text_output.is_empty() {
                    if let Some(proc) = engine.processes.get_mut(&pid) {
                        proc.syscall_buffer.push_str(&text_output);
                        pending_syscall = scan_syscall_buffer(&mut proc.syscall_buffer);
                    }
                }

                if let Some(full_command) = pending_syscall {
                    let content = full_command
                        .trim()
                        .trim_start_matches("[[")
                        .trim_end_matches("]]")
                        .trim()
                        .to_string();
                    tracing::info!(pid, owner_id, command = %full_command, "OS: SysCall intercepted");
                    dispatch_process_syscall(
                        engine,
                        memory,
                        scheduler,
                        pid,
                        &content,
                        syscall_cmd_tx,
                        pending_events,
                        tool_registry,
                    );
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
                    if let Some(proc) = engine.processes.get_mut(&pid) {
                        proc.state = ProcessState::Finished;
                    }
                }

                let turn_state = engine.processes.get(&pid).map(|proc| proc.state.clone());
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
                }
            }
            InferenceResult::Error { pid, error } => {
                in_flight.remove(&pid);
                scheduler.clear_checked_out_process(pid);
                tracing::error!(pid, %error, "Process error from worker, killing");
                if orchestrator.is_orchestrated(pid) {
                    orchestrator.mark_failed(pid, &error);
                }
                crate::services::process_runtime::release_process_resources(
                    engine, memory, scheduler, pid,
                );
                engine.processes.remove(&pid);
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
