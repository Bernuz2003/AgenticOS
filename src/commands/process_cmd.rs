use crate::protocol;
use crate::services::process_runtime::release_process_resources;

use super::context::CommandContext;
use super::metrics::log_event;

pub(crate) fn handle_term(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MISSING_PID",
            protocol::schema::ERROR,
            "TERM requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        // If in-flight, defer as pending kill (TERM ≈ graceful but process is mid-step).
        if ctx.in_flight.contains(&pid) {
            ctx.pending_kills.push(pid);
            ctx.metrics.inc_signal_count();
            log_event("process_term", ctx.client_id, Some(pid), "deferred_term_in_flight");
            let message = format!("Termination queued for in-flight PID {}", pid);
            return protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "TERM",
                protocol::schema::TERM,
                &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                Some(&message),
            );
        }
        if let Some(engine) = ctx.engine_state.as_mut() {
            if engine.terminate_process(pid) {
                release_process_resources(engine, ctx.memory, ctx.scheduler, pid);
                ctx.metrics.inc_signal_count();
                log_event("process_term", ctx.client_id, Some(pid), "graceful_termination_requested");
                let message = format!("Termination requested for PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    &ctx.request_id,
                    "TERM",
                    protocol::schema::TERM,
                    &serde_json::json!({"pid": pid, "status": "requested", "mode": "graceful"}),
                    Some(&message),
                )
            } else {
                protocol::response_protocol_err(
                    ctx.client,
                    &ctx.request_id,
                    "PID_NOT_FOUND",
                    protocol::schema::ERROR,
                    &format!("PID {} not found", pid),
                )
            }
        } else {
            protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "NO_MODEL",
                protocol::schema::ERROR,
                "No Model Loaded",
            )
        }
    } else {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "INVALID_PID",
            protocol::schema::ERROR,
            "TERM payload must be numeric PID",
        )
    }
}

pub(crate) fn handle_kill(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MISSING_PID",
            protocol::schema::ERROR,
            "KILL requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        // If the PID is currently in-flight on the inference worker, defer the kill.
        if ctx.in_flight.contains(&pid) {
            ctx.pending_kills.push(pid);
            ctx.metrics.inc_signal_count();
            log_event("process_kill", ctx.client_id, Some(pid), "deferred_kill_in_flight");
            let message = format!("Kill queued for in-flight PID {}", pid);
            return protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "KILL",
                protocol::schema::KILL,
                &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                Some(&message),
            );
        }
        if let Some(engine) = ctx.engine_state.as_mut() {
            engine.kill_process(pid);
            release_process_resources(engine, ctx.memory, ctx.scheduler, pid);
            ctx.metrics.inc_signal_count();
            log_event("process_kill", ctx.client_id, Some(pid), "killed_immediately");
            let message = format!("Killed PID {}", pid);
            protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "KILL",
                protocol::schema::KILL,
                &serde_json::json!({"pid": pid, "status": "killed", "mode": "immediate"}),
                Some(&message),
            )
        } else {
            protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "NO_MODEL",
                protocol::schema::ERROR,
                "No Model Loaded",
            )
        }
    } else {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "INVALID_PID",
            protocol::schema::ERROR,
            "KILL payload must be numeric PID",
        )
    }
}
