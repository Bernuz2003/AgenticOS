use std::sync::mpsc;

use agentic_control_models::{InvocationKind, InvocationStatus, KernelEvent};

use crate::diagnostics::audit::{self, AuditContext};
use crate::memory::NeuralMemory;
use crate::process::ProcessState;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::StorageService;

use super::invocation_events::emit_invocation_updated;
use super::worker::SyscallCompletion;

pub(crate) fn drain_syscall_results(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    result_rx: &mpsc::Receiver<SyscallCompletion>,
    pending_events: &mut Vec<KernelEvent>,
) -> usize {
    let mut processed_results = 0usize;
    while let Ok(completion) = result_rx.try_recv() {
        processed_results = processed_results.saturating_add(1);
        let pid = completion.pid;
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            tracing::warn!(
                pid,
                "OS: dropping syscall completion for unknown runtime pid"
            );
            continue;
        };
        let should_release_runtime = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                tracing::warn!(
                    pid,
                    runtime_id,
                    "OS: dropping syscall completion for unloaded runtime"
                );
                continue;
            };
            let audit_context = AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(&runtime_id),
            );

            if completion.outcome.should_kill_process {
                let _ = engine.inject_context(
                    pid,
                    &engine
                        .format_system_message(&format!("Output:\n{}", completion.outcome.output)),
                );
                kill_managed_process_with_session(
                    engine,
                    memory,
                    scheduler,
                    session_registry,
                    storage,
                    pid,
                    "syscall_killed",
                );
                pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "syscall_killed".to_string(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "syscall_killed".to_string(),
                });
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "syscall_killed".to_string(),
                });
                emit_invocation_updated(
                    pending_events,
                    pid,
                    &completion.tool_call_id,
                    InvocationKind::Tool,
                    &completion.command,
                    InvocationStatus::Killed,
                );
                audit::record(
                    storage,
                    audit::TOOL_KILLED,
                    format!(
                        "tool_call_id={} command={} caller={} transport=text duration_ms={} success={} detail={}",
                        completion.tool_call_id,
                        completion.command,
                        completion.caller.as_str(),
                        completion.outcome.duration_ms,
                        completion.outcome.success,
                        completion.outcome.output
                    ),
                    audit_context,
                );
                true
            } else {
                match engine.inject_context(
                    pid,
                    &engine
                        .format_system_message(&format!("Output:\n{}", completion.outcome.output)),
                ) {
                    Ok(()) => {
                        if let Some(process) = engine.processes.get_mut(&pid) {
                            process.state = ProcessState::Ready;
                        }
                        pending_events.push(KernelEvent::WorkspaceChanged {
                            pid,
                            reason: "syscall_completed".to_string(),
                        });
                        emit_invocation_updated(
                            pending_events,
                            pid,
                            &completion.tool_call_id,
                            InvocationKind::Tool,
                            &completion.command,
                            if completion.outcome.success {
                                InvocationStatus::Completed
                            } else {
                                InvocationStatus::Failed
                            },
                        );
                    }
                    Err(err) => {
                        tracing::warn!(pid, %err, "OS: dropping syscall completion for missing process");
                    }
                }
                let spec = if completion.outcome.success {
                    audit::TOOL_COMPLETED
                } else {
                    audit::TOOL_FAILED
                };
                audit::record(
                    storage,
                    spec,
                    format!(
                        "tool_call_id={} command={} caller={} transport=text duration_ms={} detail={}",
                        completion.tool_call_id,
                        completion.command,
                        completion.caller.as_str(),
                        completion.outcome.duration_ms,
                        completion.outcome.output
                    ),
                    audit_context,
                );
                false
            }
        };

        if should_release_runtime {
            if let Err(err) = runtime_registry.release_pid(storage, pid) {
                tracing::warn!(pid, %err, "RUNTIME: failed to release pid after syscall kill");
            }
        }
    }

    processed_results
}
