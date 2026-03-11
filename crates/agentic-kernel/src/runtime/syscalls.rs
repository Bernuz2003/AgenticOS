use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::scheduler::ProcessPriority;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::{
    kill_managed_process, spawn_managed_process, ManagedProcessRequest,
};
use crate::tool_registry::ToolRegistry;
use crate::tools::{handle_syscall, SysCallOutcome, SyscallRateMap};
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
    pub outcome: SysCallOutcome,
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
                        let outcome = match rate_map.lock() {
                            Ok(mut guard) => handle_syscall(&content, pid, &mut guard, &registry),
                            Err(_) => SysCallOutcome {
                                output: "SysCall Error: worker rate-limit state is unavailable."
                                    .to_string(),
                                should_kill_process: true,
                            },
                        };
                        if result_tx.send(SyscallCompletion { pid, outcome }).is_err() {
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
    if buffer.len() > 8000 {
        buffer.clear();
    }
    None
}

pub(super) fn dispatch_process_syscall(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    content: &str,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) {
    let quota_exceeded = scheduler.record_syscall(pid);
    if quota_exceeded {
        tracing::warn!(pid, "SCHEDULER: syscall quota exceeded — killing process");
        kill_managed_process(engine, memory, scheduler, pid);
        return;
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

        match spawn_managed_process(
            engine,
            memory,
            scheduler,
                ManagedProcessRequest {
                    prompt: prompt.to_string(),
                    owner_id,
                    workload,
                    priority,
                    lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                    context_policy: inherited_context_policy,
                },
            ) {
            Ok(new_pid) => {
                pending_events.push(KernelEvent::SessionStarted {
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
            }
            Err(e) => {
                let _ = engine
                    .inject_context(pid, &engine.format_system_message(&format!("ERROR: {}", e)));
            }
        }
    } else if content.starts_with("SEND:") {
        dispatch_send_syscall(engine, pid, content);
    } else {
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
    }
}

pub(super) fn drain_syscall_results(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    result_rx: &mpsc::Receiver<SyscallCompletion>,
    pending_events: &mut Vec<KernelEvent>,
) {
    while let Ok(completion) = result_rx.try_recv() {
        let pid = completion.pid;

        if completion.outcome.should_kill_process {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!("Output:\n{}", completion.outcome.output)),
            );
            kill_managed_process(engine, memory, scheduler, pid);
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
            continue;
        }

        match engine.inject_context(
            pid,
            &engine.format_system_message(&format!("Output:\n{}", completion.outcome.output)),
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
