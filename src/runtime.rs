use mio::{Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::prompting::{format_interprocess_user_message, format_system_injection, PromptFamily};
use crate::protocol;
use crate::tools::handle_syscall;
use crate::transport::Client;

pub fn run_engine_tick(
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    memory: &Rc<RefCell<NeuralMemory>>,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    active_family: PromptFamily,
) {
    let mut lock = engine_state.lock().unwrap();
    if let Some(engine) = lock.as_mut() {
        let swap_events = memory.borrow_mut().poll_swap_events();
        for event in swap_events {
            if event.success {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                println!(
                    "MEMORY: swap complete pid={} resumed={} detail={}",
                    event.pid, resumed, event.detail
                );
            } else {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                eprintln!(
                    "MEMORY: swap failed pid={} resumed={} detail={}",
                    event.pid, resumed, event.detail
                );
            }
        }

        let active_pids = engine.list_active_pids();

        for pid in active_pids {
            let step_result = engine.step_process(pid);
            match step_result {
                Ok(Some((text, owner_id))) => {
                    let mut pending_syscall: Option<String> = None;

                    if let Some(proc) = engine.processes.get_mut(&pid) {
                        proc.syscall_buffer.push_str(&text);

                        if let Some(start) = proc.syscall_buffer.find("[[") {
                            if let Some(end_offset) = proc.syscall_buffer[start..].find("]]") {
                                let end = start + end_offset + 2;
                                let full_command = proc.syscall_buffer[start..end].to_string();
                                proc.syscall_buffer.clear();
                                pending_syscall = Some(full_command);
                            }
                        }

                        if proc.syscall_buffer.len() > 8000 {
                            proc.syscall_buffer.clear();
                        }
                    }

                    if let Some(full_command) = pending_syscall {
                        let content = full_command[2..full_command.len() - 2].trim().to_string();
                        println!(
                            "OS: SysCall from PID {} (Owner {}): {}",
                            pid, owner_id, full_command
                        );

                        if content.starts_with("SPAWN:") {
                            let prompt = content.trim_start_matches("SPAWN:").trim();
                            match engine.spawn_process(prompt, 500, 0) {
                                Ok(new_pid) => {
                                    let msg = format!("SUCCESS: Worker Created (PID {}).\nSTOP SPAWNING NEW PROCESSES.\nNEXT ACTION: Use [[SEND: {} | <your_question>]] immediately.", new_pid, new_pid);
                                    let feedback = format_system_injection(&msg, active_family);
                                    let _ = engine.inject_context(pid, &feedback);
                                }
                                Err(e) => {
                                    let _ = engine.inject_context(
                                        pid,
                                        &format_system_injection(
                                            &format!("ERROR: {}", e),
                                            active_family,
                                        ),
                                    );
                                }
                            }
                        } else if content.starts_with("SEND:") {
                            let parts: Vec<&str> =
                                content.trim_start_matches("SEND:").splitn(2, '|').collect();
                            if parts.len() == 2 {
                                let message = parts[1].trim();
                                let target_pid_str = parts[0].trim();
                                if let Ok(target_pid) = parts[0].trim().parse::<u64>() {
                                    let msg_target = format_interprocess_user_message(
                                        pid,
                                        message,
                                        active_family,
                                    );
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
                                    let err_msg = format!("ERROR: Invalid PID format '{}'. You must use a numeric PID (e.g., [[SEND: 2 | ...]]).", target_pid_str);
                                    let _ = engine.inject_context(
                                        pid,
                                        &format_system_injection(&err_msg, active_family),
                                    );
                                }
                            }
                        } else if content.starts_with("PYTHON:")
                            || content.starts_with("WRITE_FILE:")
                            || content.starts_with("READ_FILE:")
                            || content.starts_with("LS")
                            || content.starts_with("CALC:")
                        {
                            let outcome = handle_syscall(&content, pid);
                            let _ = engine.inject_context(
                                pid,
                                &format_system_injection(
                                    &format!("Output:\n{}", outcome.output),
                                    active_family,
                                ),
                            );
                            if outcome.should_kill_process {
                                let _ = memory.borrow_mut().release_process(pid);
                                engine.kill_process(pid);
                            }
                        }
                    }

                    if owner_id > 0 {
                        let token = Token(owner_id);
                        if let Some(client) = clients.get_mut(&token) {
                            client
                                .output_buffer
                                .extend(protocol::response_data(text.as_bytes()));
                            let _ = poll.registry().reregister(
                                &mut client.stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("Error PID {}: {}", pid, e);
                    let _ = memory.borrow_mut().release_process(pid);
                    engine.kill_process(pid);
                }
            }
        }

        let finished_pids = engine.list_finished_pids();
        for pid in finished_pids {
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
            let _ = memory.borrow_mut().release_process(pid);
            engine.kill_process(pid);
        }
    }
}
