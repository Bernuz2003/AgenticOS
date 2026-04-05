use agentic_protocol::ControlErrorCode;

use crate::commands::context::ProcessCommandContext;

use super::input::SendInputPayload;
use super::resume::ensure_live_session_binding;

pub(super) struct SessionContinuationTarget {
    pub(super) session_id: String,
    pub(super) runtime_id: String,
    pub(super) pid: u64,
    pub(super) resumed_from_history: bool,
}

pub(super) fn resolve_send_input_target(
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

pub(crate) fn runtime_selector_for_session(
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
