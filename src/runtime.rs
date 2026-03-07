use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use crate::engine::LLMEngine;
use crate::inference_worker::{InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::prompting::{format_interprocess_user_message, format_system_injection, should_stop_on_text, PromptFamily};
use crate::protocol;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::tools::{handle_syscall, SyscallRateMap};
use crate::transport::Client;

/// Scan the syscall buffer for a complete `[[...]]` command.
/// Returns the full command including brackets if found.
/// Clears the buffer when a command is found or when it exceeds the safety limit.
fn scan_syscall_buffer(buffer: &mut String) -> Option<String> {
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

/// Dispatch a parsed process syscall (SPAWN, SEND, tool execution).
fn dispatch_process_syscall(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    content: &str,
    active_family: PromptFamily,
    rate_map: &mut SyscallRateMap,
) {
    // Enforce syscall quota — kill process if exceeded.
    let quota_exceeded = scheduler.record_syscall(pid);
    if quota_exceeded {
        tracing::warn!(pid, "SCHEDULER: syscall quota exceeded — killing process");
        let _ = memory.release_process(pid);
        scheduler.unregister(pid);
        engine.kill_process(pid);
        return;
    }

    if content.starts_with("SPAWN:") {
        let prompt = content.trim_start_matches("SPAWN:").trim();
        match engine.spawn_process(prompt, 500, 0) {
            Ok(new_pid) => {
                let msg = format!(
                    "SUCCESS: Worker Created (PID {}).\nSTOP SPAWNING NEW PROCESSES.\nNEXT ACTION: Use [[SEND: {} | <your_question>]] immediately.",
                    new_pid, new_pid
                );
                let feedback = format_system_injection(&msg, active_family);
                let _ = engine.inject_context(pid, &feedback);
            }
            Err(e) => {
                let _ = engine.inject_context(
                    pid,
                    &format_system_injection(&format!("ERROR: {}", e), active_family),
                );
            }
        }
    } else if content.starts_with("SEND:") {
        dispatch_send_syscall(engine, pid, content, active_family);
    } else if content.starts_with("PYTHON:")
        || content.starts_with("WRITE_FILE:")
        || content.starts_with("READ_FILE:")
        || content.starts_with("LS")
        || content.starts_with("CALC:")
    {
        let outcome = handle_syscall(content, pid, rate_map);
        let _ = engine.inject_context(
            pid,
            &format_system_injection(&format!("Output:\n{}", outcome.output), active_family),
        );
        if outcome.should_kill_process {
            let _ = memory.release_process(pid);
            engine.kill_process(pid);
        }
    }
}

/// Dispatch a SEND syscall to transfer a message between processes.
fn dispatch_send_syscall(
    engine: &mut LLMEngine,
    pid: u64,
    content: &str,
    active_family: PromptFamily,
) {
    let parts: Vec<&str> = content.trim_start_matches("SEND:").splitn(2, '|').collect();
    if parts.len() == 2 {
        let message = parts[1].trim();
        let target_pid_str = parts[0].trim();
        if let Ok(target_pid) = target_pid_str.parse::<u64>() {
            let msg_target = format_interprocess_user_message(pid, message, active_family);
            match engine.inject_context(target_pid, &msg_target) {
                Ok(_) => {
                    let _ = engine.inject_context(
                        pid,
                        &format_system_injection(
                            "MESSAGE SENT. Waiting for reply... (Do not send again).",
                            active_family,
                        ),
                    );
                }
                Err(_) => {
                    let _ = engine.inject_context(
                        pid,
                        &format_system_injection(
                            "ERROR: Target PID not found (Process does not exist).",
                            active_family,
                        ),
                    );
                }
            }
        } else {
            let err_msg = format!(
                "ERROR: Invalid PID format '{}'. You must use a numeric PID (e.g., [[SEND: 2 | ...]]).",
                target_pid_str
            );
            let _ = engine.inject_context(
                pid,
                &format_system_injection(&err_msg, active_family),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_engine_tick(
    engine_state: &mut Option<LLMEngine>,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    active_family: PromptFamily,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    result_rx: &mpsc::Receiver<InferenceResult>,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    rate_map: &mut SyscallRateMap,
) {
    if let Some(engine) = engine_state.as_mut() {
        // ── 1. Poll swap events ─────────────────────────────────────
        let swap_events = memory.poll_swap_events();
        for event in swap_events {
            if event.success {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                tracing::info!(
                    pid = event.pid,
                    resumed,
                    detail = %event.detail,
                    "MEMORY: swap complete"
                );
            } else {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                tracing::error!(
                    pid = event.pid,
                    resumed,
                    detail = %event.detail,
                    "MEMORY: swap failed"
                );
            }
        }

        // ── 2. Drain results from inference worker ──────────────────
        while let Ok(result) = result_rx.try_recv() {
            match result {
                InferenceResult::Token {
                    pid,
                    mut process,
                    token_id,
                    finished,
                } => {
                    in_flight.remove(&pid);

                    // Text-based stop check (eos/eot/max_tokens already handled by worker).
                    let text_output = engine.tokenizer.decode(&[token_id], true).ok();
                    if let Some(ref t) = text_output {
                        if !finished && should_stop_on_text(engine.family, t) {
                            process.state = crate::process::ProcessState::Finished;
                        }
                    }

                    // Re-insert process into engine.
                    engine.processes.insert(pid, process);

                    // Check pending kills (KILL issued while process was in-flight).
                    if pending_kills.contains(&pid) {
                        pending_kills.retain(|&p| p != pid);
                        let _ = memory.release_process(pid);
                        scheduler.unregister(pid);
                        engine.kill_process(pid);
                        continue;
                    }

                    let owner_id = engine.process_owner_id(pid).unwrap_or(0);

                    // Record token and enforce quota.
                    let token_quota_exceeded = scheduler.record_token(pid);

                    // Track output for orchestrated tasks.
                    if let Some(ref t) = text_output {
                        if orchestrator.is_orchestrated(pid) {
                            orchestrator.append_output(pid, t);
                        }
                    }

                    // Syscall buffer scan.
                    let mut pending_syscall: Option<String> = None;
                    if let Some(ref t) = text_output {
                        if let Some(proc) = engine.processes.get_mut(&pid) {
                            proc.syscall_buffer.push_str(t);
                            pending_syscall = scan_syscall_buffer(&mut proc.syscall_buffer);
                        }
                    }

                    if let Some(full_command) = pending_syscall {
                        let content = full_command[2..full_command.len() - 2].trim().to_string();
                        tracing::info!(
                            pid,
                            owner_id,
                            command = %full_command,
                            "OS: SysCall intercepted"
                        );
                        dispatch_process_syscall(engine, memory, scheduler, pid, &content, active_family, rate_map);
                    }

                    // Deliver token to client.
                    if let Some(ref t) = text_output {
                        if owner_id > 0 {
                            let token = Token(owner_id);
                            if let Some(client) = clients.get_mut(&token) {
                                client
                                    .output_buffer
                                    .extend(protocol::response_data(t.as_bytes()));
                                let _ = poll.registry().reregister(
                                    &mut client.stream,
                                    token,
                                    Interest::READABLE | Interest::WRITABLE,
                                );
                            }
                        }
                    }

                    // Force-terminate process if token quota exceeded.
                    if token_quota_exceeded {
                        tracing::warn!(pid, "SCHEDULER: token quota exceeded — terminating process");
                        if let Some(proc) = engine.processes.get_mut(&pid) {
                            proc.state = crate::process::ProcessState::Finished;
                        }
                    }
                }
                InferenceResult::Error { pid, error } => {
                    in_flight.remove(&pid);
                    tracing::error!(pid, %error, "Process error from worker, killing");
                    if orchestrator.is_orchestrated(pid) {
                        orchestrator.mark_failed(pid, &error);
                    }
                    let _ = memory.release_process(pid);
                    scheduler.unregister(pid);
                    // Process already dropped in the worker; just remove from engine map.
                    engine.processes.remove(&pid);
                }
            }
        }

        // ── 3. Handle finished PIDs ─────────────────────────────────
        let finished_pids = engine.list_finished_pids();
        for pid in finished_pids {
            // Notify orchestrator before cleanup.
            if orchestrator.is_orchestrated(pid) {
                orchestrator.mark_completed(pid);
            }

            if let Some(owner_id) = engine.process_owner_id(pid) {
                if owner_id > 0 {
                    let token = Token(owner_id);
                    if let Some(client) = clients.get_mut(&token) {
                        let end_msg = format!("\n[PROCESS_FINISHED pid={}]\n", pid);
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
            let _ = memory.release_process(pid);
            scheduler.unregister(pid);
            engine.kill_process(pid);
        }

        // ── 4. Checkout active processes and send to worker ─────────
        let active_pids = engine.list_active_pids();
        let ordered_pids = scheduler.scheduling_order(&active_pids);
        let eos = engine.eos_token_id;
        let eot = engine.eot_token_id;

        for pid in ordered_pids {
            if in_flight.contains(&pid) {
                continue; // Already being processed by the worker.
            }
            if let Some(process) = engine.processes.remove(&pid) {
                in_flight.insert(pid);
                let _ = cmd_tx.send(InferenceCmd::Step {
                    pid,
                    process,
                    eos_token_id: eos,
                    eot_token_id: eot,
                });
            }
        }

        // ── 5. Orchestration advance ────────────────────────────────
        let (spawn_requests, kill_pids) = orchestrator.advance();

        // Kill tasks for fail-fast policy.
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
                            &mut client.stream, token,
                            Interest::READABLE | Interest::WRITABLE,
                        );
                    }
                }
            }
            let _ = memory.release_process(pid);
            scheduler.unregister(pid);
            engine.kill_process(pid);
        }

        // Spawn tasks whose dependencies are now satisfied.
        for req in spawn_requests {
            match engine.spawn_process(&req.prompt, 0, req.owner_id) {
                Ok(pid) => {
                    if let Some(token_slots) = engine.process_max_tokens(pid) {
                        if let Err(e) = memory.register_process(pid, token_slots) {
                            engine.kill_process(pid);
                            orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, &e.to_string());
                            tracing::error!(task_id = %req.task_id, %e, "ORCHESTRATOR: memory admission failed");
                            continue;
                        }
                    }
                    scheduler.register(pid, req.workload, ProcessPriority::Normal);
                    orchestrator.register_pid(pid, req.orch_id, &req.task_id);
                    tracing::info!(
                        pid, orch_id = req.orch_id, task_id = %req.task_id,
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
}

#[cfg(test)]
mod tests {
    use super::scan_syscall_buffer;

    #[test]
    fn scan_finds_complete_command() {
        let mut buf = "some text [[PYTHON: print('hello')]] more text".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert_eq!(result, Some("[[PYTHON: print('hello')]]".to_string()));
        assert!(buf.is_empty());
    }

    #[test]
    fn scan_returns_none_for_incomplete() {
        let mut buf = "some text [[ no closing bracket".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(!buf.is_empty());
    }

    #[test]
    fn scan_clears_on_overflow() {
        let mut buf = "x".repeat(8001);
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn scan_empty_buffer_returns_none() {
        let mut buf = String::new();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
    }

    #[test]
    fn scan_only_opening_brackets() {
        let mut buf = "[[start but never ends".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(!buf.is_empty());
    }
}
