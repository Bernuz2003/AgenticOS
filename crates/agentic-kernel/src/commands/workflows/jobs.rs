use crate::commands::context::OrchestrationCommandContext;
use crate::commands::diagnostics::log_event;
use crate::protocol;
use crate::services::job_scheduler::{ScheduledJobTriggerInput, ScheduledWorkflowJobRequest};
use agentic_control_models::ScheduledJobControlResult;
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ScheduleJobPayload {
    name: String,
    workflow: serde_json::Value,
    trigger: ScheduledJobTriggerInput,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    max_retries: Option<u32>,
    #[serde(default)]
    backoff_ms: Option<u64>,
    #[serde(default = "schedule_job_enabled_default")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SetJobEnabledPayload {
    job_id: u64,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteJobPayload {
    job_id: u64,
}

fn schedule_job_enabled_default() -> bool {
    true
}

pub(crate) fn handle_schedule_job(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let payload_text = String::from_utf8_lossy(payload);
    let request = match serde_json::from_str::<ScheduleJobPayload>(payload_text.trim()) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::ScheduleJobInvalid,
                protocol::schema::ERROR,
                &format!("Invalid scheduler job JSON: {}", err),
            ));
        }
    };

    let workflow_payload = match serde_json::to_string(&request.workflow) {
        Ok(payload) => payload,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::ScheduleJobInvalid,
                protocol::schema::ERROR,
                &format!("Invalid workflow payload: {}", err),
            ));
        }
    };
    let workflow = match serde_json::from_value(request.workflow) {
        Ok(workflow) => workflow,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::ScheduleJobInvalid,
                protocol::schema::ERROR,
                &format!("Workflow definition is invalid: {}", err),
            ));
        }
    };

    match ctx.job_scheduler.schedule_workflow_job(
        ctx.storage,
        ScheduledWorkflowJobRequest {
            name: request.name.trim().to_string(),
            workflow,
            workflow_payload,
            trigger: request.trigger,
            timeout_ms: request.timeout_ms,
            max_retries: request.max_retries,
            backoff_ms: request.backoff_ms,
            enabled: request.enabled,
        },
    ) {
        Ok(result) => {
            log_event(
                "schedule_job",
                ctx.client_id,
                None,
                &format!(
                    "job_id={} trigger={} next_run_at_ms={}",
                    result.job_id,
                    result.trigger_kind,
                    result
                        .next_run_at_ms
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "none".to_string())
                ),
            );
            ctx.pending_events
                .push(agentic_control_models::KernelEvent::LobbyChanged {
                    reason: "scheduled_job_created".to_string(),
                });
            let json = serde_json::to_string(&result).expect("ScheduleJobResult is serializable");
            Some(protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "SCHEDULE_JOB",
                protocol::schema::SCHEDULE_JOB,
                &result,
                Some(&json),
            ))
        }
        Err(err) => Some(protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::ScheduleJobInvalid,
            protocol::schema::ERROR,
            &err,
        )),
    }
}

pub(crate) fn handle_set_job_enabled(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let request = match serde_json::from_slice::<SetJobEnabledPayload>(payload) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::SetJobEnabledInvalid,
                protocol::schema::ERROR,
                &format!("Invalid set job enabled payload JSON: {}", err),
            ));
        }
    };

    match ctx
        .job_scheduler
        .set_enabled(ctx.storage, request.job_id, request.enabled)
    {
        Ok(job) => {
            let result = ScheduledJobControlResult {
                job_id: job.job_id,
                enabled: job.enabled,
                state: job.state,
            };
            log_event(
                "set_job_enabled",
                ctx.client_id,
                None,
                &format!("job_id={} enabled={}", result.job_id, result.enabled),
            );
            ctx.pending_events
                .push(agentic_control_models::KernelEvent::LobbyChanged {
                    reason: "scheduled_job_mutated".to_string(),
                });
            Some(protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "SET_JOB_ENABLED",
                protocol::schema::SET_JOB_ENABLED,
                &result,
                Some(
                    &serde_json::to_string(&result)
                        .expect("ScheduledJobControlResult is serializable"),
                ),
            ))
        }
        Err(err) => Some(protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::SetJobEnabledInvalid,
            protocol::schema::ERROR,
            &err,
        )),
    }
}

pub(crate) fn handle_delete_job(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let request = match serde_json::from_slice::<DeleteJobPayload>(payload) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::DeleteJobInvalid,
                protocol::schema::ERROR,
                &format!("Invalid delete job payload JSON: {}", err),
            ));
        }
    };

    match ctx.job_scheduler.delete_job(ctx.storage, request.job_id) {
        Ok(()) => {
            let result = ScheduledJobControlResult {
                job_id: request.job_id,
                enabled: false,
                state: "deleted".to_string(),
            };
            log_event(
                "delete_job",
                ctx.client_id,
                None,
                &format!("job_id={}", result.job_id),
            );
            ctx.pending_events
                .push(agentic_control_models::KernelEvent::LobbyChanged {
                    reason: "scheduled_job_deleted".to_string(),
                });
            Some(protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "DELETE_JOB",
                protocol::schema::DELETE_JOB,
                &result,
                Some(
                    &serde_json::to_string(&result)
                        .expect("ScheduledJobControlResult is serializable"),
                ),
            ))
        }
        Err(err) => Some(protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::DeleteJobInvalid,
            protocol::schema::ERROR,
            &err,
        )),
    }
}
