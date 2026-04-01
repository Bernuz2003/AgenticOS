use crate::commands::context::ProcessCommandContext;
use crate::model_catalog::parse_workload_label;
use crate::process::ProcessLifecyclePolicy;
use crate::prompting::{format_initial_prompt_with_metadata, format_user_message_with_metadata};
use crate::protocol;
use crate::scheduler::ProcessPriority;
use crate::services::model_runtime::activate_model_target;
use crate::services::process_runtime::{
    spawn_restored_managed_process_with_session, RestoredManagedProcessRequest,
};
use crate::storage::StoredReplayMessage;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use agentic_control_models::{KernelEvent, ResumeSessionResult};
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use super::targeting::{runtime_selector_for_session, SessionContinuationTarget};

#[derive(Deserialize)]
struct ResumeSessionPayload {
    session_id: String,
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

pub(super) fn ensure_live_session_binding(
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
                format!(
                    "Runtime '{}' is not available for session resume",
                    runtime_id
                ),
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
    crate::commands::diagnostics::log_event(
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
            "assistant" if message.kind != "thinking" => {
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
