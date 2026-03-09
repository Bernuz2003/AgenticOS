use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::scheduler::ProcessScheduler;
use crate::scheduler::ProcessPriority;
use crate::services::process_runtime::spawn_managed_process;
use crate::services::process_runtime::kill_managed_process;
use crate::tool_registry::ToolRegistry;
use crate::tools::{handle_syscall, SyscallRateMap};

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
    rate_map: &mut SyscallRateMap,
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

        match spawn_managed_process(
            engine,
            memory,
            scheduler,
            prompt,
            owner_id,
            workload,
            priority,
        ) {
            Ok(new_pid) => {
                let msg = format!(
                    "SUCCESS: Worker Created (PID {}).\nSTOP SPAWNING NEW PROCESSES.\nNEXT ACTION: Use [[SEND: {} | <your_question>]] immediately.",
                    new_pid.pid, new_pid.pid
                );
                let feedback = engine.format_system_message(&msg);
                let _ = engine.inject_context(pid, &feedback);
            }
            Err(e) => {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(&format!("ERROR: {}", e)),
                );
            }
        }
    } else if content.starts_with("SEND:") {
        dispatch_send_syscall(engine, pid, content);
    } else {
        let outcome = handle_syscall(content, pid, rate_map, tool_registry);
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(&format!("Output:\n{}", outcome.output)),
        );
        if outcome.should_kill_process {
            kill_managed_process(engine, memory, scheduler, pid);
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