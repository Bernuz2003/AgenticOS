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
    pid: u64,
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

    let had_pending_human_request = process.pending_human_request.is_some();

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

pub(crate) fn handle_resume_session(ctx: ProcessCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
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

    if let Some(active_pid) = ctx.session_registry.active_pid_for_session(session_id) {
        return protocol::response_protocol_ok(
            ctx.client,
            ctx.request_id,
            "RESUME_SESSION",
            agentic_protocol::schema::RESUME_SESSION,
            &ResumeSessionResult {
                session_id: session_id.to_string(),
                pid: active_pid,
                resumed_from_history: false,
            },
            Some(&format!(
                "Session {} is already bound to live PID {}",
                session_id, active_pid
            )),
        );
    }

    let Some(session_record) = ctx.session_registry.session(session_id).cloned() else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::Generic,
            protocol::schema::ERROR,
            &format!("Session '{}' not found", session_id),
        );
    };

    let mut runtime_id = session_record.runtime_id.clone().or_else(|| {
        ctx.runtime_registry
            .current_runtime_id()
            .map(ToString::to_string)
    });

    let Some(candidate_runtime_id) = runtime_id.clone() else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            &format!(
                "Session '{}' has no persisted runtime binding and no runtime is currently loaded",
                session_id
            ),
        );
    };

    if !ctx
        .runtime_registry
        .is_runtime_loaded(&candidate_runtime_id)
    {
        let selector =
            match runtime_selector_for_session(ctx.runtime_registry, &candidate_runtime_id) {
                Ok(selector) => selector,
                Err(detail) => {
                    return protocol::response_protocol_err_typed(
                        ctx.client,
                        ctx.request_id,
                        ControlErrorCode::NoModel,
                        protocol::schema::ERROR,
                        &detail,
                    );
                }
            };

        if let Err(err) = ctx.model_catalog.refresh() {
            tracing::warn!(
                session_id,
                runtime_id = candidate_runtime_id,
                %err,
                "PROCESS_CMD: failed to refresh model catalog before session resume"
            );
        }

        let target = match ctx.model_catalog.resolve_load_target(&selector) {
            Ok(target) => target,
            Err(err) => {
                return protocol::response_protocol_err_typed(
                    ctx.client,
                    ctx.request_id,
                    ControlErrorCode::LoadFailed,
                    protocol::schema::ERROR,
                    &format!(
                        "Failed to resolve runtime '{}' for session '{}': {}",
                        candidate_runtime_id, session_id, err
                    ),
                );
            }
        };

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
                return protocol::response_protocol_err_typed(
                    ctx.client,
                    ctx.request_id,
                    ControlErrorCode::LoadFailed,
                    protocol::schema::ERROR,
                    err.message(),
                );
            }
        }
    }

    let Some(runtime_id) = runtime_id else {
        return protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        );
    };

    let replay_messages = match ctx.storage.load_replay_messages_for_session(session_id) {
        Ok(messages) => messages,
        Err(err) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::Generic,
                protocol::schema::ERROR,
                &format!(
                    "Failed to load persisted history for session '{}': {}",
                    session_id, err
                ),
            );
        }
    };

    let system_prompt =
        crate::agent_prompt::build_agent_system_prompt(ctx.tool_registry, ToolCaller::AgentText);
    let rendered_prompt = {
        let Some(engine) = ctx.runtime_registry.engine(&runtime_id) else {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                &format!(
                    "Runtime '{}' is not loaded after activation for session '{}'",
                    runtime_id, session_id
                ),
            );
        };

        match render_prompt_from_replay_history(&replay_messages, &system_prompt, engine) {
            Ok(prompt) => prompt,
            Err(detail) => {
                return protocol::response_protocol_err_typed(
                    ctx.client,
                    ctx.request_id,
                    ControlErrorCode::Generic,
                    protocol::schema::ERROR,
                    &detail,
                );
            }
        }
    };

    let workload = ctx
        .storage
        .latest_workload_for_session(session_id)
        .ok()
        .flatten()
        .and_then(|value| parse_workload_label(&value))
        .unwrap_or_default();
    let permission_policy = match ProcessPermissionPolicy::interactive_chat(ctx.tool_registry) {
        Ok(policy) => policy,
        Err(err) => {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::SpawnFailed,
                protocol::schema::ERROR,
                &err,
            );
        }
    };
    let pid_floor = ctx.runtime_registry.next_pid_floor();

    let spawn_result = {
        let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
            return protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::NoModel,
                protocol::schema::ERROR,
                &format!(
                    "Runtime '{}' is not available for session resume",
                    runtime_id
                ),
            );
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
    };

    match spawn_result {
        Ok(spawned) => {
            if let Err(err) =
                ctx.runtime_registry
                    .register_pid(ctx.storage, &runtime_id, spawned.pid)
            {
                tracing::warn!(
                    pid = spawned.pid,
                    runtime_id,
                    %err,
                    "PROCESS_CMD: failed to register resumed pid in runtime registry"
                );
            }

            ctx.pending_events.push(KernelEvent::WorkspaceChanged {
                pid: spawned.pid,
                reason: "session_resumed".to_string(),
            });
            ctx.pending_events.push(KernelEvent::LobbyChanged {
                reason: "session_resumed".to_string(),
            });
            log_event(
                "process_resume_session",
                ctx.client_id,
                Some(spawned.pid),
                "session_resumed_from_persisted_history",
            );

            protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "RESUME_SESSION",
                agentic_protocol::schema::RESUME_SESSION,
                &ResumeSessionResult {
                    session_id: spawned.session_id,
                    pid: spawned.pid,
                    resumed_from_history: true,
                },
                Some(&format!(
                    "Session {} resumed on PID {}",
                    session_id, spawned.pid
                )),
            )
        }
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::SpawnFailed,
            protocol::schema::ERROR,
            &err,
        ),
    }
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
