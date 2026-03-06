use crate::protocol;

use super::context::CommandContext;
use super::metrics::{inc_signal_count, log_event};

pub(crate) fn handle_term(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_err_code("MISSING_PID", "TERM requires PID payload")
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let mut lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
        if let Some(engine) = lock.as_mut() {
            if engine.terminate_process(pid) {
                let _ = ctx.memory.borrow_mut().release_process(pid);
                ctx.scheduler.unregister(pid);
                inc_signal_count();
                log_event("process_term", ctx.client_id, Some(pid), "graceful_termination_requested");
                protocol::response_ok_code("TERM", &format!("Termination requested for PID {}", pid))
            } else {
                protocol::response_err_code("PID_NOT_FOUND", &format!("PID {} not found", pid))
            }
        } else {
            protocol::response_err_code("NO_MODEL", "No Model Loaded")
        }
    } else {
        protocol::response_err_code("INVALID_PID", "TERM payload must be numeric PID")
    }
}

pub(crate) fn handle_kill(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_err_code("MISSING_PID", "KILL requires PID payload")
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let mut lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
        if let Some(engine) = lock.as_mut() {
            engine.kill_process(pid);
            let _ = ctx.memory.borrow_mut().release_process(pid);
            ctx.scheduler.unregister(pid);
            inc_signal_count();
            log_event("process_kill", ctx.client_id, Some(pid), "killed_immediately");
            protocol::response_ok_code("KILL", &format!("Killed PID {}", pid))
        } else {
            protocol::response_err_code("NO_MODEL", "No Model Loaded")
        }
    } else {
        protocol::response_err_code("INVALID_PID", "KILL payload must be numeric PID")
    }
}
