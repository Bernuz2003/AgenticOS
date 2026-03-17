use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::scheduler::ProcessPriority;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_managed_process_with_session, ManagedProcessRequest,
};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::ToolCaller;
use crate::tools::{handle_syscall, SysCallOutcome, SyscallRateMap};
use crate::{audit, audit::AuditContext};
use agentic_control_models::KernelEvent;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use super::actions::{self, ActionInvocation, ActionName};

#[derive(Debug)]
pub(crate) enum SyscallCmd {
    Execute {
        pid: u64,
        content: String,
        caller: ToolCaller,
        registry: ToolRegistry,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct SyscallCompletion {
    pub pid: u64,
    pub command: String,
    pub caller: ToolCaller,
    pub outcome: SysCallOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyscallDispatchOutcome {
    None,
    Spawned(u64),
    Killed,
}

pub(crate) fn spawn_syscall_worker(
    rate_map: Arc<Mutex<SyscallRateMap>>,
    result_tx: mpsc::Sender<SyscallCompletion>,
    cmd_rx: mpsc::Receiver<SyscallCmd>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("syscall-worker".into())
        .spawn(move || {
            while let Ok(command) = cmd_rx.recv() {
                match command {
                    SyscallCmd::Execute {
                        pid,
                        content,
                        caller,
                        registry,
                    } => {
                        let command = content.clone();
                        let outcome = match rate_map.lock() {
                            Ok(mut guard) => {
                                handle_syscall(&content, pid, caller.clone(), &mut guard, &registry)
                            }
                            Err(_) => SysCallOutcome {
                                output: "SysCall Error: worker rate-limit state is unavailable."
                                    .to_string(),
                                success: false,
                                duration_ms: 0,
                                should_kill_process: true,
                            },
                        };
                        if result_tx
                            .send(SyscallCompletion {
                                pid,
                                command,
                                caller,
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    SyscallCmd::Shutdown => break,
                }
            }
        })
        .expect("failed to spawn syscall worker")
}

pub(super) fn scan_syscall_buffer(buffer: &mut String) -> Option<String> {
    let mut absolute_offset = 0usize;
    for line in buffer.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();
        let marker_offset = absolute_offset + leading_ws;

        for prefix in ["ACTION:", "TOOL:"] {
            if !trimmed.starts_with(prefix) {
                continue;
            }

            let candidate = &buffer[marker_offset..];
            match crate::text_invocation::extract_prefixed_json_invocation(candidate, prefix) {
                crate::text_invocation::PrefixedInvocationExtract::Parsed(parsed) => {
                    let consumed = marker_offset + parsed.consumed_bytes;
                    let command = parsed.raw_invocation;
                    buffer.drain(..consumed);
                    return Some(command);
                }
                crate::text_invocation::PrefixedInvocationExtract::Incomplete => return None,
                crate::text_invocation::PrefixedInvocationExtract::Invalid(_) => {
                    let newline_rel = candidate.find('\n');
                    let command_end = newline_rel.unwrap_or(candidate.len());
                    let command = candidate[..command_end].trim_end_matches('\r').to_string();
                    let consumed = marker_offset + newline_rel.map_or(command_end, |idx| idx + 1);
                    buffer.drain(..consumed);
                    return Some(command);
                }
            }
        }

        absolute_offset += line.len();
    }

    if buffer.len() > 8000 {
        buffer.clear();
    }
    None
}

pub(super) fn dispatch_process_syscall(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    content: &str,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    let caller = engine
        .processes
        .get(&pid)
        .map(|process| process.tool_caller.clone())
        .unwrap_or(ToolCaller::AgentText);

    let quota_exceeded = scheduler.record_syscall(pid);
    if quota_exceeded {
        tracing::warn!(pid, "SCHEDULER: syscall quota exceeded — killing process");
        kill_managed_process_with_session(
            engine,
            memory,
            scheduler,
            session_registry,
            storage,
            pid,
            "syscall_quota_exceeded",
        );
        return SyscallDispatchOutcome::Killed;
    }

    if content.starts_with("ACTION:") {
        if !caller.can_orchestrate_actions() {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: this session is interactive and cannot orchestrate other agents.",
                ),
            );
            return SyscallDispatchOutcome::None;
        }

        match actions::parse_text_invocation(content) {
            Ok(invocation) => dispatch_action_invocation(
                runtime_id,
                pid_floor,
                engine,
                memory,
                scheduler,
                pid,
                invocation,
                session_registry,
                storage,
                pending_events,
                tool_registry,
            ),
            Err(err) => {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(&format!("ACTION Error: {}", err)),
                );
                SyscallDispatchOutcome::None
            }
        }
    } else {
        audit::record(
            storage,
            audit::TOOL_DISPATCHED,
            format!("command={content} caller={} transport=text", caller.as_str()),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let queued = syscall_cmd_tx.send(SyscallCmd::Execute {
            pid,
            content: content.to_string(),
            caller,
            registry: tool_registry.clone(),
        });
        match queued {
            Ok(()) => {
                if let Some(process) = engine.processes.get_mut(&pid) {
                    process.state = ProcessState::WaitingForSyscall;
                }
            }
            Err(err) => {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(&format!(
                        "SysCall Error: failed to enqueue execution: {}",
                        err
                    )),
                );
                if let Some(process) = engine.processes.get_mut(&pid) {
                    process.state = ProcessState::Ready;
                }
            }
        }
        SyscallDispatchOutcome::None
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_action_invocation(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    invocation: ActionInvocation,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    match invocation.action {
        ActionName::Spawn => {
            let Some(prompt) = invocation
                .input
                .get("prompt")
                .and_then(|value| value.as_str())
            else {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:spawn requires string field 'prompt'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            };
            dispatch_spawn_action(
                runtime_id,
                pid_floor,
                engine,
                memory,
                scheduler,
                pid,
                prompt,
                session_registry,
                storage,
                pending_events,
                tool_registry,
            )
        }
        ActionName::Send => {
            let Some(target_pid) = invocation.input.get("pid").and_then(|value| value.as_u64())
            else {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:send requires numeric field 'pid'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            };
            let Some(message) = invocation
                .input
                .get("message")
                .and_then(|value| value.as_str())
            else {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:send requires string field 'message'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            };
            dispatch_send_action(engine, pid, target_pid, message);
            SyscallDispatchOutcome::None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_spawn_action(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    prompt: &str,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        let _ = engine.inject_context(
            pid,
            &engine
                .format_system_message("ACTION Error: ACTION:spawn requires a non-empty 'prompt'."),
        );
        return SyscallDispatchOutcome::None;
    }

    let owner_id = engine.process_owner_id(pid).unwrap_or(0);
    let parent_sched = scheduler.snapshot(pid);
    let workload = parent_sched
        .as_ref()
        .map(|snapshot| snapshot.workload)
        .unwrap_or(WorkloadClass::General);
    let priority = parent_sched
        .as_ref()
        .map(|snapshot| snapshot.priority)
        .unwrap_or(ProcessPriority::Normal);
    let inherited_context_policy = engine
        .processes
        .get(&pid)
        .map(|process| process.context_policy.clone());

    if runtime_id.is_empty() {
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message("ERROR: Runtime binding not found for SPAWN syscall."),
        );
        return SyscallDispatchOutcome::None;
    }

    match spawn_managed_process_with_session(
        runtime_id,
        pid_floor,
        engine,
        memory,
        scheduler,
        session_registry,
        storage,
        ManagedProcessRequest {
            prompt: prompt.to_string(),
            system_prompt: Some(crate::agent_prompt::build_agent_system_prompt(
                tool_registry,
                ToolCaller::AgentText,
            )),
            owner_id,
            tool_caller: ToolCaller::AgentText,
            workload,
            required_backend_class: None,
            priority,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: inherited_context_policy,
        },
    ) {
        Ok(new_pid) => {
            pending_events.push(KernelEvent::SessionStarted {
                session_id: new_pid.session_id.clone(),
                pid: new_pid.pid,
                workload: format!("{:?}", workload).to_lowercase(),
                prompt: prompt.to_string(),
            });
            pending_events.push(KernelEvent::WorkspaceChanged {
                pid: new_pid.pid,
                reason: "syscall_spawned".to_string(),
            });
            pending_events.push(KernelEvent::LobbyChanged {
                reason: "syscall_spawned".to_string(),
            });
            let msg = format!("SUCCESS: Worker Created (PID {}).", new_pid.pid);
            let feedback = engine.format_system_message(&msg);
            let _ = engine.inject_context(pid, &feedback);
            SyscallDispatchOutcome::Spawned(new_pid.pid)
        }
        Err(e) => {
            let _ =
                engine.inject_context(pid, &engine.format_system_message(&format!("ERROR: {}", e)));
            SyscallDispatchOutcome::None
        }
    }
}

pub(super) fn drain_syscall_results(
    runtime_registry: &mut crate::runtimes::RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    result_rx: &mpsc::Receiver<SyscallCompletion>,
    pending_events: &mut Vec<KernelEvent>,
) {
    while let Ok(completion) = result_rx.try_recv() {
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
                audit::record(
                    storage,
                    audit::TOOL_KILLED,
                    format!(
                        "command={} caller={} transport=text duration_ms={} success={} detail={}",
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
                        "command={} caller={} transport=text duration_ms={} detail={}",
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
}

fn dispatch_send_action(engine: &mut LLMEngine, pid: u64, target_pid: u64, message: &str) {
    let message = message.trim();
    if message.is_empty() {
        let _ = engine.inject_context(
            pid,
            &engine
                .format_system_message("ACTION Error: ACTION:send requires a non-empty 'message'."),
        );
        return;
    }

    let msg_target = engine.format_interprocess_message(pid, message);
    match engine.inject_context(target_pid, &msg_target) {
        Ok(_) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "MESSAGE SENT. Waiting for reply... (Do not send again).",
                ),
            );
        }
        Err(_) => {
            let _ = engine.inject_context(
                pid,
                &engine
                    .format_system_message("ERROR: Target PID not found (Process does not exist)."),
            );
        }
    }
}
