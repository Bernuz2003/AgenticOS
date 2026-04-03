use crate::diagnostics::audit::{self, AuditContext};
use crate::protocol;
use crate::services::process_control::{
    request_process_kill_with_session, request_process_termination_with_session,
    ProcessSignalResult,
};
use agentic_control_models::KernelEvent;
use agentic_protocol::ControlErrorCode;

use super::super::context::ProcessCommandContext;
use super::super::diagnostics::log_event;

pub(crate) fn handle_term(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::MissingPid,
            protocol::schema::ERROR,
            "TERM requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let audit_context = AuditContext::for_process(
            ctx.session_registry.session_id_for_pid(pid),
            pid,
            ctx.runtime_registry
                .runtime_id_for_pid(pid)
                .or_else(|| ctx.session_registry.runtime_id_for_pid(pid)),
        );
        match request_process_termination_with_session(
            ctx.runtime_registry,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            ctx.in_flight,
            ctx.pending_kills,
            pid,
        ) {
            ProcessSignalResult::Deferred => {
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "term_queued".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_term",
                    ctx.client_id,
                    Some(pid),
                    "deferred_term_in_flight",
                );
                let message = format!("Termination queued for in-flight PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "TERM",
                    protocol::schema::TERM,
                    &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::Applied => {
                ctx.pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "terminated".to_string(),
                });
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "terminated".to_string(),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "terminated".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_term",
                    ctx.client_id,
                    Some(pid),
                    "graceful_termination_requested",
                );
                audit::record(
                    ctx.storage,
                    audit::PROCESS_TERMINATED,
                    "mode=graceful",
                    audit_context,
                );
                let message = format!("Termination requested for PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "TERM",
                    protocol::schema::TERM,
                    &serde_json::json!({"pid": pid, "status": "requested", "mode": "graceful"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::NotFound => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not found", pid),
            ),
            ProcessSignalResult::NoModelLoaded => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                "No Model Loaded",
            ),
        }
    } else {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidPid,
            protocol::schema::ERROR,
            "TERM payload must be numeric PID",
        )
    }
}

pub(crate) fn handle_kill(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::MissingPid,
            protocol::schema::ERROR,
            "KILL requires PID payload",
        )
    } else if let Ok(pid) = payload_text.parse::<u64>() {
        let audit_context = AuditContext::for_process(
            ctx.session_registry.session_id_for_pid(pid),
            pid,
            ctx.runtime_registry
                .runtime_id_for_pid(pid)
                .or_else(|| ctx.session_registry.runtime_id_for_pid(pid)),
        );
        match request_process_kill_with_session(
            ctx.runtime_registry,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            ctx.in_flight,
            ctx.pending_kills,
            pid,
        ) {
            ProcessSignalResult::Deferred => {
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "kill_queued".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_kill",
                    ctx.client_id,
                    Some(pid),
                    "deferred_kill_in_flight",
                );
                let message = format!("Kill queued for in-flight PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "KILL",
                    protocol::schema::KILL,
                    &serde_json::json!({"pid": pid, "status": "queued", "mode": "deferred"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::Applied => {
                ctx.pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "killed".to_string(),
                });
                ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "killed".to_string(),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "killed".to_string(),
                });
                ctx.metrics.inc_signal_count();
                log_event(
                    "process_kill",
                    ctx.client_id,
                    Some(pid),
                    "killed_immediately",
                );
                audit::record(
                    ctx.storage,
                    audit::PROCESS_KILLED,
                    "mode=immediate",
                    audit_context,
                );
                let message = format!("Killed PID {}", pid);
                protocol::response_protocol_ok(
                    ctx.client,
                    ctx.request_id,
                    "KILL",
                    protocol::schema::KILL,
                    &serde_json::json!({"pid": pid, "status": "killed", "mode": "immediate"}),
                    Some(&message),
                )
            }
            ProcessSignalResult::NotFound => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not found", pid),
            ),
            ProcessSignalResult::NoModelLoaded => protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                "No Model Loaded",
            ),
        }
    } else {
        protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidPid,
            protocol::schema::ERROR,
            "KILL payload must be numeric PID",
        )
    }
}
