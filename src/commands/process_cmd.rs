use crate::protocol;
use crate::services::process_runtime::release_process_resources;

use super::context::CommandContext;
use super::metrics::log_event;

pub(crate) fn handle_term(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_err_code("MISSING_PID", "TERM requires PID payload")
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        // If in-flight, defer as pending kill (TERM ≈ graceful but process is mid-step).
        if ctx.in_flight.contains(&pid) {
            ctx.pending_kills.push(pid);
            ctx.metrics.inc_signal_count();
            log_event("process_term", ctx.client_id, Some(pid), "deferred_term_in_flight");
            return protocol::response_ok_code("TERM", &format!("Termination queued for in-flight PID {}", pid));
        }
        if let Some(engine) = ctx.engine_state.as_mut() {
            if engine.terminate_process(pid) {
                release_process_resources(engine, ctx.memory, ctx.scheduler, pid);
                ctx.metrics.inc_signal_count();
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
        // If the PID is currently in-flight on the inference worker, defer the kill.
        if ctx.in_flight.contains(&pid) {
            ctx.pending_kills.push(pid);
            ctx.metrics.inc_signal_count();
            log_event("process_kill", ctx.client_id, Some(pid), "deferred_kill_in_flight");
            return protocol::response_ok_code("KILL", &format!("Kill queued for in-flight PID {}", pid));
        }
        if let Some(engine) = ctx.engine_state.as_mut() {
            engine.kill_process(pid);
            release_process_resources(engine, ctx.memory, ctx.scheduler, pid);
            ctx.metrics.inc_signal_count();
            log_event("process_kill", ctx.client_id, Some(pid), "killed_immediately");
            protocol::response_ok_code("KILL", &format!("Killed PID {}", pid))
        } else {
            protocol::response_err_code("NO_MODEL", "No Model Loaded")
        }
    } else {
        protocol::response_err_code("INVALID_PID", "KILL payload must be numeric PID")
    }
}
