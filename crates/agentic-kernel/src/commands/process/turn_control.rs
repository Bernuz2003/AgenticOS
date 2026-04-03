use agentic_control_models::{KernelEvent, TurnControlResult};
use agentic_protocol::ControlErrorCode;

use crate::diagnostics::audit::{self, AuditContext};
use crate::protocol;
use crate::runtime::AssistantTurnRuntimeBoundary;

use super::super::context::ProcessCommandContext;
use super::super::diagnostics::log_event;
use super::input::PidPayload;

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
    if ctx.in_flight.contains(&payload.pid) {
        ctx.turn_assembly.request_output_stop(payload.pid);
        ctx.pending_events.push(KernelEvent::WorkspaceChanged {
            pid: payload.pid,
            reason: "output_stop_requested".to_string(),
        });
        ctx.pending_events.push(KernelEvent::LobbyChanged {
            reason: "output_stop_requested".to_string(),
        });
        log_event(
            "process_stop_output",
            ctx.client_id,
            Some(payload.pid),
            "soft_stop_requested_for_in_flight_turn",
        );
        return protocol::response_protocol_ok(
            ctx.client,
            ctx.request_id,
            "STOP_OUTPUT",
            agentic_protocol::schema::STOP_OUTPUT,
            &TurnControlResult {
                pid: payload.pid,
                state: "running".to_string(),
                action: "request_stop_output".to_string(),
            },
            Some(&format!(
                "Stop requested for PID {}. The runtime will stop at the next safe boundary.",
                payload.pid
            )),
        );
    }

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
                "PID {} cannot stop output in state {:?}; use it while running or awaiting a turn decision",
                payload.pid, process.state
            ),
        );
    }

    match engine.stop_current_turn(payload.pid) {
        Ok(()) => {
            if let Some(turn_id) = ctx.session_registry.active_turn_id_for_pid(payload.pid) {
                if let Some(segments) = ctx.turn_assembly.drain_pending_segments(payload.pid) {
                    for segment in segments {
                        if let Err(err) = ctx.storage.append_assistant_segment(
                            turn_id,
                            segment.kind,
                            &segment.text,
                        ) {
                            tracing::warn!(
                                pid = payload.pid,
                                turn_id,
                                %err,
                                "PROCESS_CMD: failed to persist pending assistant segment before STOP_OUTPUT"
                            );
                        }
                    }
                }
                if let Err(err) =
                    ctx.storage
                        .finish_turn(turn_id, "completed", "output_stopped", None)
                {
                    tracing::warn!(
                        pid = payload.pid,
                        turn_id,
                        %err,
                        "PROCESS_CMD: failed to persist STOP_OUTPUT turn finish"
                    );
                } else {
                    ctx.session_registry.clear_active_turn(payload.pid);
                    ctx.turn_assembly.apply_runtime_boundary(
                        payload.pid,
                        AssistantTurnRuntimeBoundary::RuntimeClosed,
                    );
                }
            } else {
                tracing::warn!(
                    pid = payload.pid,
                    "PROCESS_CMD: active turn missing during STOP_OUTPUT"
                );
                ctx.turn_assembly.apply_runtime_boundary(
                    payload.pid,
                    AssistantTurnRuntimeBoundary::RuntimeClosed,
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
