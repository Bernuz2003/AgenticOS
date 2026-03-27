use crate::protocol;
use crate::diagnostics::audit::{self, AuditContext};
use agentic_control_models::{KernelEvent, SendInputResult, TurnControlResult};
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use crate::commands::context::ProcessCommandContext;

use super::targeting::resolve_send_input_target;

#[derive(Deserialize)]
pub(super) struct SendInputPayload {
    #[serde(default)]
    pub(super) pid: Option<u64>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    pub(super) prompt: String,
}

#[derive(Deserialize)]
pub(super) struct PidPayload {
    pub(super) pid: u64,
}

pub(crate) fn handle_send_input(mut ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
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
                    "SEND_INPUT expects JSON payload {{\"pid\":...,\"session_id\":\"...\",\"prompt\":\"...\"}}: {}",
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

    let target = match resolve_send_input_target(&mut ctx, &payload) {
        Ok(target) => target,
        Err((code, detail)) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                code,
                protocol::schema::ERROR,
                &detail,
            );
        }
    };

    let Some(engine) = ctx.runtime_registry.engine_mut(&target.runtime_id) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let Some(process) = engine.processes.get(&target.pid) else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::PidNotFound,
            protocol::schema::ERROR,
            &format!("PID {} not found", target.pid),
        );
    };

    let had_pending_human_request = process.pending_human_request.is_some();

    if !matches!(
        process.state,
        crate::process::ProcessState::WaitingForInput
            | crate::process::ProcessState::WaitingForHumanInput
    ) {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::InvalidSessionState,
            protocol::schema::ERROR,
            &format!(
                "PID {} is not waiting for input (state={:?})",
                target.pid, process.state
            ),
        );
    }

    match engine.send_user_input(target.pid, prompt) {
        Ok(()) => {
            let session_id = ctx
                .session_registry
                .session_id_for_pid(target.pid)
                .map(ToString::to_string);
            let workload = ctx
                .scheduler
                .snapshot(target.pid)
                .map(|snapshot| format!("{:?}", snapshot.workload).to_lowercase())
                .unwrap_or_else(|| "general".to_string());
            if let Some(session_id_ref) = session_id.as_deref() {
                match ctx.storage.start_session_turn(
                    session_id_ref,
                    target.pid,
                    &workload,
                    "send_input",
                    prompt,
                    "input",
                ) {
                    Ok(turn_id) => ctx.session_registry.remember_active_turn(target.pid, turn_id),
                    Err(err) => {
                        tracing::warn!(
                            pid = target.pid,
                            session_id = session_id_ref,
                            %err,
                            "PROCESS_CMD: failed to persist SEND_INPUT turn"
                        );
                    }
                }
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: target.pid,
                reason: "input_received".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "input_received".to_string(),
            });
            audit::record(
                ctx.storage,
                if had_pending_human_request {
                    audit::PROCESS_HUMAN_INPUT_RECEIVED
                } else {
                    audit::PROCESS_INPUT_RECEIVED
                },
                format!(
                    "source=send_input chars={} hitl={}",
                    prompt.chars().count(),
                    had_pending_human_request
                ),
                AuditContext::for_process(
                    session_id.as_deref(),
                    target.pid,
                    Some(&target.runtime_id),
                ),
            );
            crate::commands::diagnostics::log_event(
                "process_continue",
                ctx.client_id,
                Some(target.pid),
                if target.resumed_from_history {
                    "input_appended_after_implicit_session_resume"
                } else {
                    "input_appended_to_live_session"
                },
            );
            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "SEND_INPUT",
                agentic_protocol::schema::SEND_INPUT,
                &SendInputResult {
                    pid: target.pid,
                    state: "ready".to_string(),
                },
                Some(&format!(
                    "Input queued for PID {}{}",
                    target.pid,
                    if target.resumed_from_history {
                        " (session resumed from history)"
                    } else {
                        ""
                    }
                )),
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
            if let Some(turn_id) = ctx.session_registry.active_turn_id_for_pid(payload.pid) {
                if let Err(err) = ctx.storage.resume_turn(turn_id) {
                    tracing::warn!(
                        pid = payload.pid,
                        turn_id,
                        %err,
                        "PROCESS_CMD: failed to persist CONTINUE_OUTPUT turn resume"
                    );
                }
            } else {
                tracing::warn!(
                    pid = payload.pid,
                    "PROCESS_CMD: active turn missing during CONTINUE_OUTPUT"
                );
            }
            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: payload.pid,
                reason: "output_continued".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "output_continued".to_string(),
            });
            crate::commands::diagnostics::log_event(
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
