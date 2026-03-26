use crate::protocol;
use crate::services::model_runtime::activate_model_target;
use crate::services::process_control::{
    request_process_kill_with_session, request_process_termination_with_session,
    ProcessSignalResult,
};
use crate::services::process_runtime::{
    spawn_restored_managed_process_with_session, RestoredManagedProcessRequest,
};
use crate::{audit, audit::AuditContext};
use agentic_control_models::{
    KernelEvent, ResumeSessionResult, SendInputResult, TurnControlResult,
};
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use super::context::ProcessCommandContext;
use super::metrics::log_event;
use crate::model_catalog::parse_workload_label;
use crate::process::ProcessLifecyclePolicy;
use crate::prompting::{format_initial_prompt_with_metadata, format_user_message_with_metadata};
use crate::scheduler::ProcessPriority;
use crate::storage::StoredReplayMessage;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};

#[derive(Deserialize)]
struct SendInputPayload {
    #[serde(default)]
    pid: Option<u64>,
    #[serde(default)]
    session_id: Option<String>,
    prompt: String,
}

#[derive(Deserialize)]
struct PidPayload {
    pid: u64,
}

#[derive(Deserialize)]
struct ResumeSessionPayload {
    session_id: String,
}

struct SessionContinuationTarget {
    session_id: String,
    runtime_id: String,
    pid: u64,
    resumed_from_history: bool,
}

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
            log_event(
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
            if let Some(turn_id) = ctx.session_registry.active_turn_id_for_pid(payload.pid) {
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
                }
            } else {
                tracing::warn!(
                    pid = payload.pid,
                    "PROCESS_CMD: active turn missing during STOP_OUTPUT"
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

pub(crate) fn handle_resume_session(mut ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload = match serde_json::from_slice::<ResumeSessionPayload>(payload)
        .map_err(|err| err.to_string())
    {
        Ok(value) => value,
        Err(detail) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::ResumeSessionInvalid,
                protocol::schema::ERROR,
                &format!(
                    "RESUME_SESSION expects JSON payload {{\"session_id\":\"...\"}}: {}",
                    detail
                ),
            );
        }
    };

    let session_id = payload.session_id.trim();
    if session_id.is_empty() {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::ResumeSessionInvalid,
            protocol::schema::ERROR,
            "RESUME_SESSION requires a non-empty session_id",
        );
    }

    match ensure_live_session_binding(&mut ctx, session_id) {
        Ok(target) => protocol::response_protocol_ok(
            ctx.client,
            ctx.request_id,
            "RESUME_SESSION",
            agentic_protocol::schema::RESUME_SESSION,
            &ResumeSessionResult {
                session_id: target.session_id.clone(),
                pid: target.pid,
                resumed_from_history: target.resumed_from_history,
            },
            Some(&format!(
                "Session {} {} PID {}",
                session_id,
                if target.resumed_from_history {
                    "resumed on"
                } else {
                    "is already bound to live"
                },
                target.pid
            )),
        ),
        Err((code, detail)) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            code,
            protocol::schema::ERROR,
            &detail,
        ),
    }
}

fn resolve_send_input_target(
    ctx: &mut ProcessCommandContext<'_>,
    payload: &SendInputPayload,
) -> Result<SessionContinuationTarget, (ControlErrorCode, String)> {
    if let Some(pid) = payload.pid {
        let runtime_id = ctx
            .runtime_registry
            .runtime_id_for_pid(pid)
            .or_else(|| ctx.session_registry.runtime_id_for_pid(pid))
            .map(ToString::to_string);
        let has_live_process = runtime_id
            .as_deref()
            .and_then(|runtime_id| ctx.runtime_registry.engine(runtime_id))
            .is_some_and(|engine| engine.processes.contains_key(&pid));

        if has_live_process {
            return Ok(SessionContinuationTarget {
                session_id: ctx
                    .session_registry
                    .session_id_for_pid(pid)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("pid-{pid}")),
                runtime_id: runtime_id.unwrap_or_default(),
                pid,
                resumed_from_history: false,
            });
        }

        if payload.session_id.is_none() {
            return Err((
                if runtime_id.is_some() {
                    ControlErrorCode::PidNotFound
                } else {
                    ControlErrorCode::NoModel
                },
                if runtime_id.is_some() {
                    format!("PID {} not found", pid)
                } else {
                    "No Model Loaded".to_string()
                },
            ));
        }
    }

    let session_id = payload
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                ControlErrorCode::SendInputInvalid,
                "SEND_INPUT requires either pid or session_id".to_string(),
            )
        })?;
    ensure_live_session_binding(ctx, session_id)
}

fn ensure_live_session_binding(
    ctx: &mut ProcessCommandContext<'_>,
    session_id: &str,
) -> Result<SessionContinuationTarget, (ControlErrorCode, String)> {
    if let Some(active_pid) = ctx.session_registry.active_pid_for_session(session_id) {
        let runtime_id = ctx
            .runtime_registry
            .runtime_id_for_pid(active_pid)
            .or_else(|| ctx.session_registry.runtime_id_for_pid(active_pid))
            .map(ToString::to_string);
        let has_live_process = runtime_id
            .as_deref()
            .and_then(|runtime_id| ctx.runtime_registry.engine(runtime_id))
            .is_some_and(|engine| engine.processes.contains_key(&active_pid));

        if has_live_process {
            return Ok(SessionContinuationTarget {
                session_id: session_id.to_string(),
                runtime_id: runtime_id.unwrap_or_default(),
                pid: active_pid,
                resumed_from_history: false,
            });
        }

        if let Err(err) = ctx
            .session_registry
            .release_pid(ctx.storage, active_pid, "interrupted")
        {
            tracing::warn!(
                session_id,
                pid = active_pid,
                %err,
                "PROCESS_CMD: failed to clear stale live binding before session resume"
            );
        }
    }

    let Some(session_record) = ctx.session_registry.session(session_id).cloned() else {
        return Err((
            ControlErrorCode::Generic,
            format!("Session '{}' not found", session_id),
        ));
    };

    let mut runtime_id = session_record.runtime_id.clone().or_else(|| {
        ctx.runtime_registry
            .current_runtime_id()
            .map(ToString::to_string)
    });

    let Some(candidate_runtime_id) = runtime_id.clone() else {
        return Err((
            ControlErrorCode::NoModel,
            format!(
                "Session '{}' has no persisted runtime binding and no runtime is currently loaded",
                session_id
            ),
        ));
    };

    if !ctx
        .runtime_registry
        .is_runtime_loaded(&candidate_runtime_id)
    {
        let selector = runtime_selector_for_session(ctx.runtime_registry, &candidate_runtime_id)
            .map_err(|detail| (ControlErrorCode::NoModel, detail))?;

        if let Err(err) = ctx.model_catalog.refresh() {
            tracing::warn!(
                session_id,
                runtime_id = candidate_runtime_id,
                %err,
                "PROCESS_CMD: failed to refresh model catalog before session resume"
            );
        }

        let target = ctx
            .model_catalog
            .resolve_load_target(&selector)
            .map_err(|err| {
                (
                    ControlErrorCode::LoadFailed,
                    format!(
                        "Failed to resolve runtime '{}' for session '{}': {}",
                        candidate_runtime_id, session_id, err
                    ),
                )
            })?;

        match activate_model_target(
            ctx.runtime_registry,
            ctx.resource_governor,
            ctx.session_registry,
            ctx.storage,
            ctx.model_catalog,
            &target,
        ) {
            Ok(loaded) => {
                runtime_id = Some(loaded.runtime_id);
            }
            Err(err) => {
                return Err((ControlErrorCode::LoadFailed, err.message().to_string()));
            }
        }
    }

    let Some(runtime_id) = runtime_id else {
        return Err((ControlErrorCode::NoModel, "No Model Loaded".to_string()));
    };

    let replay_messages = ctx
        .storage
        .load_replay_messages_for_session(session_id)
        .map_err(|err| {
            (
                ControlErrorCode::Generic,
                format!(
                    "Failed to load persisted history for session '{}': {}",
                    session_id, err
                ),
            )
        })?;

    let system_prompt =
        crate::agent_prompt::build_agent_system_prompt(ctx.tool_registry, ToolCaller::AgentText);
    let rendered_prompt = {
        let Some(engine) = ctx.runtime_registry.engine(&runtime_id) else {
            return Err((
                ControlErrorCode::NoModel,
                format!(
                    "Runtime '{}' is not loaded after activation for session '{}'",
                    runtime_id, session_id
                ),
            ));
        };

        render_prompt_from_replay_history(&replay_messages, &system_prompt, engine)
            .map_err(|detail| (ControlErrorCode::Generic, detail))?
    };

    let workload = ctx
        .storage
        .latest_workload_for_session(session_id)
        .ok()
        .flatten()
        .and_then(|value| parse_workload_label(&value))
        .unwrap_or_default();
    let permission_policy = ProcessPermissionPolicy::interactive_chat(ctx.tool_registry)
        .map_err(|err| (ControlErrorCode::SpawnFailed, err))?;
    let pid_floor = ctx.runtime_registry.next_pid_floor();

    let spawn_result = {
        let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
            return Err((
                ControlErrorCode::NoModel,
                format!("Runtime '{}' is not available for session resume", runtime_id),
            ));
        };

        spawn_restored_managed_process_with_session(
            &runtime_id,
            session_id,
            pid_floor,
            engine,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            RestoredManagedProcessRequest {
                rendered_prompt,
                owner_id: ctx.client_id,
                tool_caller: ToolCaller::AgentText,
                permission_policy: Some(permission_policy),
                workload,
                required_backend_class: None,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: None,
            },
        )
        .map_err(|err| (ControlErrorCode::SpawnFailed, err))
    }?;

    if let Err(err) = ctx
        .runtime_registry
        .register_pid(ctx.storage, &runtime_id, spawn_result.pid)
    {
        tracing::warn!(
            pid = spawn_result.pid,
            runtime_id,
            %err,
            "PROCESS_CMD: failed to register resumed pid in runtime registry"
        );
    }

    ctx.pending_events.push(KernelEvent::WorkspaceChanged {
        pid: spawn_result.pid,
        reason: "session_resumed".to_string(),
    });
    ctx.pending_events.push(KernelEvent::LobbyChanged {
        reason: "session_resumed".to_string(),
    });
    log_event(
        "process_resume_session",
        ctx.client_id,
        Some(spawn_result.pid),
        "session_resumed_from_persisted_history",
    );

    Ok(SessionContinuationTarget {
        session_id: spawn_result.session_id,
        runtime_id,
        pid: spawn_result.pid,
        resumed_from_history: true,
    })
}

fn runtime_selector_for_session(
    runtime_registry: &crate::runtimes::RuntimeRegistry,
    runtime_id: &str,
) -> Result<String, String> {
    let Some(descriptor) = runtime_registry.descriptor(runtime_id) else {
        return Err(format!(
            "Persisted runtime '{}' is not present in the runtime registry",
            runtime_id
        ));
    };

    if descriptor.target_kind == "remote_provider" {
        let Some(provider_id) = descriptor.provider_id.as_deref() else {
            return Err(format!(
                "Runtime '{}' is missing provider metadata required for remote resume",
                runtime_id
            ));
        };
        let model_id = descriptor
            .remote_model_id
            .as_deref()
            .unwrap_or(descriptor.logical_model_id.as_str());
        return Ok(format!("cloud:{provider_id}:{model_id}"));
    }

    if descriptor.target_kind == "local_path" {
        return Ok(descriptor.display_path.clone());
    }

    if !descriptor.logical_model_id.trim().is_empty() {
        return Ok(descriptor.logical_model_id.clone());
    }

    Ok(descriptor.display_path.clone())
}

fn render_prompt_from_replay_history(
    replay_messages: &[StoredReplayMessage],
    system_prompt: &str,
    engine: &crate::engine::LLMEngine,
) -> Result<String, String> {
    let mut rendered = String::new();
    let mut saw_user_message = false;

    for message in replay_messages {
        match message.role.as_str() {
            "user" => {
                if !saw_user_message {
                    rendered.push_str(&format_initial_prompt_with_metadata(
                        Some(system_prompt),
                        &message.content,
                        engine.loaded_family(),
                        engine.model_metadata(),
                    ));
                    saw_user_message = true;
                } else {
                    rendered.push_str(&format_user_message_with_metadata(
                        &message.content,
                        engine.loaded_family(),
                        engine.model_metadata(),
                    ));
                }
            }
            "assistant" => {
                rendered.push_str(&message.content);
            }
            _ => {}
        }
    }

    if !saw_user_message {
        return Err("Session has no persisted user messages to reconstruct for resume".to_string());
    }

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::handle_send_input;
    use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
    use crate::commands::context::ProcessCommandContext;
    use crate::commands::metrics::MetricsState;
    use crate::config::OpenAIResponsesConfig;
    use crate::memory::NeuralMemory;
    use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
    use crate::process::ProcessLifecyclePolicy;
    use crate::prompting::PromptFamily;
    use crate::resource_governor::ResourceGovernor;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::scheduler::{ProcessPriority, ProcessScheduler};
    use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use crate::tool_registry::ToolRegistry;
    use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
    use crate::transport::Client;

    #[test]
    fn send_input_by_session_id_implicitly_resumes_historical_session() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let base = temp_dir("agenticos_process_cmd_resume");
        let db_path = base.join("agenticos.db");

        let session_id = {
            let mut storage = StorageService::open(&db_path).expect("open storage");
            let boot = storage
                .record_kernel_boot("0.5.0-test")
                .expect("record first boot");
            let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("load runtimes");
            let mut session_registry =
                SessionRegistry::load(&mut storage, boot.boot_id).expect("load sessions");
            let mut memory = NeuralMemory::new().expect("memory init");
            let mut scheduler = ProcessScheduler::new();
            let tool_registry = ToolRegistry::with_builtins();

            let runtime = runtime_registry
                .activate_target(
                    &mut storage,
                    &remote_target(),
                    RuntimeReservation::default(),
                )
                .expect("activate remote runtime");
            let spawned = {
                let pid_floor = runtime_registry.next_pid_floor();
                let engine = runtime_registry
                    .engine_mut(&runtime.runtime_id)
                    .expect("runtime engine");
                spawn_managed_process_with_session(
                    &runtime.runtime_id,
                    pid_floor,
                    engine,
                    &mut memory,
                    &mut scheduler,
                    &mut session_registry,
                    &mut storage,
                    ManagedProcessRequest {
                        prompt: "Prima domanda".to_string(),
                        system_prompt: None,
                        owner_id: 41,
                        tool_caller: ToolCaller::AgentText,
                        permission_policy: Some(
                            ProcessPermissionPolicy::interactive_chat(&tool_registry)
                                .expect("interactive permissions"),
                        ),
                        workload: WorkloadClass::General,
                        required_backend_class: None,
                        priority: ProcessPriority::Normal,
                        lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                        context_policy: None,
                    },
                )
                .expect("spawn initial session")
            };
            runtime_registry
                .register_pid(&mut storage, &runtime.runtime_id, spawned.pid)
                .expect("register initial pid");

            let turn_id = storage
                .start_session_turn(
                    &spawned.session_id,
                    spawned.pid,
                    "general",
                    "test",
                    "Prima domanda",
                    "prompt",
                )
                .expect("start first turn");
            session_registry.remember_active_turn(spawned.pid, turn_id);
            storage
                .append_assistant_message(turn_id, "Prima risposta")
                .expect("persist assistant message");
            storage
                .finish_turn(turn_id, "completed", "turn_completed", None)
                .expect("finish first turn");
            session_registry.clear_active_turn(spawned.pid);
            session_registry
                .release_pid(&mut storage, spawned.pid, "completed")
                .expect("release session pid");

            spawned.session_id
        };

        let mut storage = StorageService::open(&db_path).expect("reopen storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record second boot");
        let mut runtime_registry = RuntimeRegistry::load(&mut storage).expect("reload runtimes");
        let mut session_registry =
            SessionRegistry::load(&mut storage, boot.boot_id).expect("reload sessions");
        let mut model_catalog =
            ModelCatalog::discover(&repository_root().join("models")).expect("discover catalog");
        let mut resource_governor =
            ResourceGovernor::load(&mut storage, Default::default()).expect("load governor");
        let mut memory = NeuralMemory::new().expect("memory init");
        let mut scheduler = ProcessScheduler::new();
        let in_flight = HashSet::new();
        let mut pending_kills = Vec::new();
        let mut pending_events = Vec::new();
        let mut metrics = MetricsState::new();
        let tool_registry = ToolRegistry::with_builtins();
        let mut client = test_client();

        let payload = serde_json::to_vec(&json!({
            "session_id": session_id,
            "prompt": "Seconda domanda"
        }))
        .expect("serialize payload");

        let response = handle_send_input(
            ProcessCommandContext {
                client: &mut client,
                request_id: "test:1",
                runtime_registry: &mut runtime_registry,
                resource_governor: &mut resource_governor,
                model_catalog: &mut model_catalog,
                memory: &mut memory,
                scheduler: &mut scheduler,
                in_flight: &in_flight,
                pending_kills: &mut pending_kills,
                pending_events: &mut pending_events,
                metrics: &mut metrics,
                client_id: 99,
                session_registry: &mut session_registry,
                storage: &mut storage,
                tool_registry: &tool_registry,
            },
            &payload,
        );

        assert!(
            response.starts_with(b"+OK"),
            "expected +OK response, got: {}",
            String::from_utf8_lossy(&response)
        );

        let resumed_pid = session_registry
            .active_pid_for_session(&session_id)
            .expect("session bound to resumed pid");
        let runtime_id = session_registry
            .runtime_id_for_session(&session_id)
            .expect("runtime id for resumed session")
            .to_string();
        let process = runtime_registry
            .engine(&runtime_id)
            .and_then(|engine| engine.processes.get(&resumed_pid))
            .expect("resumed process");

        assert!(process.prompt_text().contains("Prima domanda"));
        assert!(process.prompt_text().contains("Prima risposta"));
        assert!(process.prompt_text().contains("Seconda domanda"));

        let replay_messages = storage
            .load_replay_messages_for_session(&session_id)
            .expect("load replay messages");
        assert!(replay_messages
            .iter()
            .any(|message| message.content == "Seconda domanda"));
    }

    fn test_openai_config() -> OpenAIResponsesConfig {
        OpenAIResponsesConfig {
            endpoint: "https://api.openai.example/v1/responses".to_string(),
            api_key: "test-key".to_string(),
            default_model: "gpt-4.1-mini".to_string(),
            stream: true,
            ..Default::default()
        }
    }

    fn remote_target() -> ResolvedModelTarget {
        let driver_resolution =
            resolve_driver_for_model(PromptFamily::Unknown, None, Some("openai-responses"))
                .expect("resolve remote backend");
        ResolvedModelTarget::remote(
            "openai-responses",
            "OpenAI",
            "openai-responses",
            "gpt-4.1-mini",
            RemoteModelEntry {
                id: "gpt-4.1-mini".to_string(),
                label: "GPT-4.1 mini".to_string(),
                context_window_tokens: None,
                max_output_tokens: None,
                supports_structured_output: true,
                input_price_usd_per_mtok: None,
                output_price_usd_per_mtok: None,
            },
            test_openai_config().into(),
            None,
            driver_resolution,
        )
    }

    fn test_client() -> Client {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let join = std::thread::spawn(move || listener.accept().expect("accept client").0);
        let client_stream = std::net::TcpStream::connect(addr).expect("connect listener");
        let _server_stream = join.join().expect("join accept thread");
        Client::new(mio::net::TcpStream::from_std(client_stream), true)
    }

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonical repository root")
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
