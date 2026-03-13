use crate::protocol;
use crate::services::process_control::{
    request_process_kill_with_session, request_process_termination_with_session,
    ProcessSignalResult,
};
use crate::{audit, audit::AuditContext};
use agentic_control_models::{KernelEvent, SendInputResult, TurnControlResult};
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use super::context::ProcessCommandContext;
use super::metrics::log_event;

#[derive(Deserialize)]
struct SendInputPayload {
    pid: u64,
    prompt: String,
}

#[derive(Deserialize)]
struct PidPayload {
    pid: u64,
}

pub(crate) fn handle_term(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if payload_text.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
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

pub(crate) fn handle_send_input(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let parsed = serde_json::from_slice::<SendInputPayload>(payload).map_err(|err| err.to_string());
    let payload = match parsed {
        Ok(value) => value,
        Err(detail) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::SendInputInvalid,
                protocol::schema::ERROR,
                &format!(
                    "SEND_INPUT expects JSON payload {{\"pid\":...,\"prompt\":\"...\"}}: {}",
                    detail
                ),
            );
        }
    };

    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::MissingPrompt,
            protocol::schema::ERROR,
            "SEND_INPUT requires a non-empty prompt",
        );
    }

    let Some(runtime_id) = ctx
        .runtime_registry
        .runtime_id_for_pid(payload.pid)
        .or_else(|| ctx.session_registry.runtime_id_for_pid(payload.pid))
        .map(ToString::to_string)
    else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };
    let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let Some(process) = engine.processes.get(&payload.pid) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", payload.pid),
        );
    };

    if process.state != crate::process::ProcessState::WaitingForInput {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &format!(
                "PID {} is not waiting for input (state={:?})",
                payload.pid, process.state
            ),
        );
    }

    match engine.send_user_input(payload.pid, prompt) {
        Ok(()) => {
            let session_id = ctx
                .session_registry
                .session_id_for_pid(payload.pid)
                .map(ToString::to_string);
            let workload = ctx
                .scheduler
                .snapshot(payload.pid)
                .map(|snapshot| format!("{:?}", snapshot.workload).to_lowercase())
                .unwrap_or_else(|| "general".to_string());
            if let Some(session_id_ref) = session_id.as_deref() {
                if let Err(err) = ctx.storage.start_session_turn(
                    session_id_ref,
                    payload.pid,
                    &workload,
                    "send_input",
                    prompt,
                    "input",
                ) {
                    tracing::warn!(
                        pid = payload.pid,
                        session_id = session_id_ref,
                        %err,
                        "PROCESS_CMD: failed to persist SEND_INPUT turn"
                    );
                }
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: payload.pid,
                reason: "input_received".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "input_received".to_string(),
            });
            audit::record(
                ctx.storage,
                audit::PROCESS_INPUT_RECEIVED,
                format!("source=send_input chars={}", prompt.chars().count()),
                AuditContext::for_process(session_id.as_deref(), payload.pid, Some(&runtime_id)),
            );
            log_event(
                "process_continue",
                ctx.client_id,
                Some(payload.pid),
                "input_appended_to_resident_session",
            );
            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "SEND_INPUT",
                agentic_protocol::schema::SEND_INPUT,
                &SendInputResult {
                    pid: payload.pid,
                    state: "ready".to_string(),
                },
                Some(&format!("Input queued for PID {}", payload.pid)),
            )
        }
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}

pub(crate) fn handle_continue_output(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload = match serde_json::from_slice::<PidPayload>(payload).map_err(|err| err.to_string())
    {
        Ok(value) => value,
        Err(detail) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::ContinueOutputInvalid,
                protocol::schema::ERROR,
                &format!(
                    "CONTINUE_OUTPUT expects JSON payload {{\"pid\":...}}: {}",
                    detail
                ),
            );
        }
    };

    let Some(runtime_id) = ctx
        .runtime_registry
        .runtime_id_for_pid(payload.pid)
        .or_else(|| ctx.session_registry.runtime_id_for_pid(payload.pid))
        .map(ToString::to_string)
    else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };
    let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let Some(process) = engine.processes.get(&payload.pid) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", payload.pid),
        );
    };

    if process.state != crate::process::ProcessState::AwaitingTurnDecision {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &format!(
                "PID {} is not awaiting a turn decision (state={:?})",
                payload.pid, process.state
            ),
        );
    }

    match engine.continue_current_turn(payload.pid) {
        Ok(()) => {
            if let Err(err) = ctx.storage.resume_latest_turn_for_pid(payload.pid) {
                tracing::warn!(
                    pid = payload.pid,
                    %err,
                    "PROCESS_CMD: failed to persist CONTINUE_OUTPUT turn resume"
                );
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: payload.pid,
                reason: "output_continued".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "output_continued".to_string(),
            });
            log_event(
                "process_continue_output",
                ctx.client_id,
                Some(payload.pid),
                "continuing_truncated_assistant_turn",
            );
            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "CONTINUE_OUTPUT",
                agentic_protocol::schema::CONTINUE_OUTPUT,
                &TurnControlResult {
                    pid: payload.pid,
                    state: "ready".to_string(),
                    action: "continue_output".to_string(),
                },
                Some(&format!("Continuing output for PID {}", payload.pid)),
            )
        }
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}

pub(crate) fn handle_stop_output(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload = match serde_json::from_slice::<PidPayload>(payload).map_err(|err| err.to_string())
    {
        Ok(value) => value,
        Err(detail) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::StopOutputInvalid,
                protocol::schema::ERROR,
                &format!(
                    "STOP_OUTPUT expects JSON payload {{\"pid\":...}}: {}",
                    detail
                ),
            );
        }
    };

    let Some(runtime_id) = ctx
        .runtime_registry
        .runtime_id_for_pid(payload.pid)
        .or_else(|| ctx.session_registry.runtime_id_for_pid(payload.pid))
        .map(ToString::to_string)
    else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };
    let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let Some(process) = engine.processes.get(&payload.pid) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", payload.pid),
        );
    };

    if process.state != crate::process::ProcessState::AwaitingTurnDecision {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &format!(
                "PID {} is not awaiting a turn decision (state={:?})",
                payload.pid, process.state
            ),
        );
    }

    match engine.stop_current_turn(payload.pid) {
        Ok(()) => {
            if let Err(err) = ctx.storage.finish_latest_turn_for_pid(
                payload.pid,
                "completed",
                "output_stopped",
                None,
            ) {
                tracing::warn!(
                    pid = payload.pid,
                    %err,
                    "PROCESS_CMD: failed to persist STOP_OUTPUT turn finish"
                );
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: payload.pid,
                reason: "output_stopped".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "output_stopped".to_string(),
            });
            audit::record(
                ctx.storage,
                audit::PROCESS_TURN_COMPLETED,
                "reason=output_stopped",
                AuditContext::for_process(
                    ctx.session_registry.session_id_for_pid(payload.pid),
                    payload.pid,
                    Some(&runtime_id),
                ),
            );
            log_event(
                "process_stop_output",
                ctx.client_id,
                Some(payload.pid),
                "truncated_assistant_turn_confirmed_as_stopped",
            );
            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "STOP_OUTPUT",
                agentic_protocol::schema::STOP_OUTPUT,
                &TurnControlResult {
                    pid: payload.pid,
                    state: "waiting_for_input".to_string(),
                    action: "stop_output".to_string(),
                },
                Some(&format!("Stopped output for PID {}", payload.pid)),
            )
        }
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}
