use crate::protocol;
use crate::services::job_scheduler::{ScheduledJobTriggerInput, ScheduledWorkflowJobRequest};
use crate::services::orchestration_runtime::{
    retry_orchestration_task, start_orchestration, OrchestrationRetryError, OrchestrationStartError,
};
use agentic_control_models::OrchestrateResult;
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use super::context::OrchestrationCommandContext;
use super::metrics::log_event;

#[derive(Debug, Deserialize)]
struct RetryTaskPayload {
    orchestration_id: u64,
    task_id: String,
}

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

fn schedule_job_enabled_default() -> bool {
    true
}

/// Handle the ORCHESTRATE opcode.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer, or `Some(response)` on success.
pub(crate) fn handle_orchestrate(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let payload_text = String::from_utf8_lossy(payload);
    match serde_json::from_str::<crate::orchestrator::TaskGraphDef>(payload_text.trim()) {
        Ok(graph) => {
            match start_orchestration(
                ctx.runtime_registry,
                ctx.resource_governor,
                ctx.memory,
                ctx.model_catalog,
                ctx.scheduler,
                ctx.orchestrator,
                ctx.session_registry,
                ctx.storage,
                ctx.pending_events,
                ctx.tool_registry,
                ctx.client_id,
                graph,
            ) {
                Ok(started) => {
                    for _ in 0..started.spawned {
                        ctx.metrics.inc_exec_started();
                    }
                    log_event(
                        "orchestrate",
                        ctx.client_id,
                        None,
                        &format!(
                            "orch_id={} total={} spawned={}",
                            started.orchestration_id, started.total_tasks, started.spawned
                        ),
                    );
                    let result = OrchestrateResult {
                        orchestration_id: started.orchestration_id,
                        total_tasks: started.total_tasks,
                        spawned: started.spawned,
                    };
                    Some(protocol::response_protocol_ok(
                        ctx.client,
                        ctx.request_id,
                        "ORCHESTRATE",
                        protocol::schema::ORCHESTRATE,
                        &result,
                        Some(
                            &serde_json::to_string(&result)
                                .expect("OrchestrateResult is serializable"),
                        ),
                    ))
                }
                Err(OrchestrationStartError::NoModelLoaded) => {
                    Some(protocol::response_protocol_err_typed(
                        ctx.client,
                        ctx.request_id,
                        ControlErrorCode::NoModel,
                        protocol::schema::ERROR,
                        "No Model Loaded — ORCHESTRATE requires a loaded engine",
                    ))
                }
                Err(OrchestrationStartError::InvalidGraph(err)) => {
                    Some(protocol::response_protocol_err_typed(
                        ctx.client,
                        ctx.request_id,
                        ControlErrorCode::OrchestrateInvalid,
                        protocol::schema::ERROR,
                        &err.to_string(),
                    ))
                }
                Err(OrchestrationStartError::RoutingFailed(err)) => {
                    Some(protocol::response_protocol_err_typed(
                        ctx.client,
                        ctx.request_id,
                        ControlErrorCode::OrchestrateInvalid,
                        protocol::schema::ERROR,
                        &err,
                    ))
                }
            }
        }
        Err(e) => Some(protocol::response_protocol_err_typed(
            ctx.client,
            ctx.request_id,
            ControlErrorCode::OrchestrateJson,
            protocol::schema::ERROR,
            &format!("Invalid task graph JSON: {}", e),
        )),
    }
}

pub(crate) fn handle_retry_task(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let payload_text = String::from_utf8_lossy(payload);
    let request = match serde_json::from_str::<RetryTaskPayload>(payload_text.trim()) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::RetryTaskInvalid,
                protocol::schema::ERROR,
                &format!("Invalid retry payload JSON: {}", err),
            ));
        }
    };

    match retry_orchestration_task(
        ctx.runtime_registry,
        ctx.resource_governor,
        ctx.memory,
        ctx.model_catalog,
        ctx.scheduler,
        ctx.orchestrator,
        ctx.session_registry,
        ctx.storage,
        ctx.pending_events,
        ctx.tool_registry,
        request.orchestration_id,
        &request.task_id,
    ) {
        Ok(result) => {
            for _ in 0..result.spawned {
                ctx.metrics.inc_exec_started();
            }
            log_event(
                "retry_task",
                ctx.client_id,
                None,
                &format!(
                    "orch_id={} task={} reset={} spawned={}",
                    result.orchestration_id,
                    result.task,
                    result.reset_tasks.join(","),
                    result.spawned
                ),
            );
            Some(protocol::response_protocol_ok(
                ctx.client,
                ctx.request_id,
                "RETRY_TASK",
                protocol::schema::RETRY_TASK,
                &result,
                Some(&serde_json::to_string(&result).expect("RetryTaskResult is serializable")),
            ))
        }
        Err(OrchestrationRetryError::InvalidTask(err)) => {
            Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::RetryTaskInvalid,
                protocol::schema::ERROR,
                &err.to_string(),
            ))
        }
        Err(OrchestrationRetryError::RoutingFailed(err)) => {
            Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::RetryTaskInvalid,
                protocol::schema::ERROR,
                &err,
            ))
        }
    }
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
