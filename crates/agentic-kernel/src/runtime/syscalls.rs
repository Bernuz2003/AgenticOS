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
use crate::tools::{handle_syscall, SysCallOutcome, SyscallRateMap};
use crate::{audit, audit::AuditContext};
use agentic_control_models::KernelEvent;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug)]
pub(crate) enum SyscallCmd {
    Execute {
        pid: u64,
        content: String,
        registry: ToolRegistry,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct SyscallCompletion {
    pub pid: u64,
    pub command: String,
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
                        registry,
                    } => {
                        let command = content.clone();
                        let outcome = match rate_map.lock() {
                            Ok(mut guard) => handle_syscall(&content, pid, &mut guard, &registry),
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
    if let Some(start) = buffer.find("[[") {
        if let Some(end_offset) = buffer[start..].find("]]") {
            let end = start + end_offset + 2;
            let full_command = buffer[start..end].to_string();
            buffer.clear();
            return Some(full_command);
        }
    }
    if let Some(command) = buffer
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("TOOL:") && crate::tools::validates_tool_invocation(line))
        .map(str::to_string)
    {
        buffer.clear();
        return Some(command);
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

    if content.starts_with("SPAWN:") {
        let prompt = content.trim_start_matches("SPAWN:").trim();
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
                &engine
                    .format_system_message("ERROR: Runtime binding not found for SPAWN syscall."),
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
                owner_id,
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
                let msg = format!(
                    "SUCCESS: Worker Created (PID {}).\nSTOP SPAWNING NEW PROCESSES.\nNEXT ACTION: Use [[SEND: {} | <your_question>]] immediately.",
                    new_pid.pid, new_pid.pid
                );
                let feedback = engine.format_system_message(&msg);
                let _ = engine.inject_context(pid, &feedback);
                SyscallDispatchOutcome::Spawned(new_pid.pid)
            }
            Err(e) => {
                let _ = engine
                    .inject_context(pid, &engine.format_system_message(&format!("ERROR: {}", e)));
                SyscallDispatchOutcome::None
            }
        }
    } else if content.starts_with("SEND:") {
        dispatch_send_syscall(engine, pid, content);
        SyscallDispatchOutcome::None
    } else {
        audit::record(
            storage,
            audit::TOOL_DISPATCHED,
            format!("command={content}"),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let queued = syscall_cmd_tx.send(SyscallCmd::Execute {
            pid,
            content: content.to_string(),
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
                        "command={} duration_ms={} success={} detail={}",
                        completion.command,
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
                        "command={} duration_ms={} detail={}",
                        completion.command,
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

fn dispatch_send_syscall(engine: &mut LLMEngine, pid: u64, content: &str) {
    let parts: Vec<&str> = content.trim_start_matches("SEND:").splitn(2, '|').collect();
    if parts.len() == 2 {
        let message = parts[1].trim();
        let target_pid_str = parts[0].trim();
        if let Ok(target_pid) = target_pid_str.parse::<u64>() {
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
                        &engine.format_system_message(
                            "ERROR: Target PID not found (Process does not exist).",
                        ),
                    );
                }
            }
        } else {
            let err_msg = format!(
                "ERROR: Invalid PID format '{}'. You must use a numeric PID (e.g., [[SEND: 2 | ...]]).",
                target_pid_str
            );
            let _ = engine.inject_context(pid, &engine.format_system_message(&err_msg));
        }
    }
}
