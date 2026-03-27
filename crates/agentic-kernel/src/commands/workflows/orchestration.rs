use crate::protocol;
use crate::services::orchestration_runtime::{
    delete_orchestration, retry_orchestration_task, start_orchestration, stop_orchestration,
    OrchestrationControlError, OrchestrationRetryError, OrchestrationStartError,
};
use agentic_control_models::{OrchestrateResult, OrchestrationControlResult};
use agentic_protocol::ControlErrorCode;
use serde::Deserialize;

use crate::commands::context::OrchestrationCommandContext;
use crate::commands::diagnostics::log_event;

#[derive(Debug, Deserialize)]
struct RetryTaskPayload {
    orchestration_id: u64,
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct OrchestrationControlPayload {
    orchestration_id: u64,
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

pub(crate) fn handle_stop_orchestration(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let request = match serde_json::from_slice::<OrchestrationControlPayload>(payload) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::StopOrchestrationInvalid,
                protocol::schema::ERROR,
                &format!("Invalid stop orchestration payload JSON: {}", err),
            ));
        }
    };

    match stop_orchestration(
        ctx.runtime_registry,
        ctx.memory,
        ctx.scheduler,
        ctx.orchestrator,
        ctx.job_scheduler,
        ctx.session_registry,
        ctx.storage,
        ctx.in_flight,
        ctx.pending_kills,
        ctx.pending_events,
        request.orchestration_id,
    ) {
        Ok(result) => Some(orchestration_control_ok(
            ctx,
            "STOP_ORCHESTRATION",
            protocol::schema::STOP_ORCHESTRATION,
            "stop_orchestration",
            result,
        )),
        Err(err) => Some(orchestration_control_err(
            ctx,
            ControlErrorCode::StopOrchestrationInvalid,
            err,
        )),
    }
}

pub(crate) fn handle_delete_orchestration(
    ctx: OrchestrationCommandContext<'_>,
    payload: &[u8],
) -> Option<Vec<u8>> {
    let request = match serde_json::from_slice::<OrchestrationControlPayload>(payload) {
        Ok(request) => request,
        Err(err) => {
            return Some(protocol::response_protocol_err_typed(
                ctx.client,
                ctx.request_id,
                ControlErrorCode::DeleteOrchestrationInvalid,
                protocol::schema::ERROR,
                &format!("Invalid delete orchestration payload JSON: {}", err),
            ));
        }
    };

    match delete_orchestration(
        ctx.orchestrator,
        ctx.session_registry,
        ctx.storage,
        ctx.pending_events,
        request.orchestration_id,
    ) {
        Ok(result) => Some(orchestration_control_ok(
            ctx,
            "DELETE_ORCHESTRATION",
            protocol::schema::DELETE_ORCHESTRATION,
            "delete_orchestration",
            result,
        )),
        Err(err) => Some(orchestration_control_err(
            ctx,
            ControlErrorCode::DeleteOrchestrationInvalid,
            err,
        )),
    }
}

fn orchestration_control_ok(
    ctx: OrchestrationCommandContext<'_>,
    code: &str,
    schema_id: &str,
    metric_name: &str,
    result: OrchestrationControlResult,
) -> Vec<u8> {
    log_event(
        metric_name,
        ctx.client_id,
        None,
        &format!(
            "orchestration_id={} status={}",
            result.orchestration_id, result.status
        ),
    );
    protocol::response_protocol_ok(
        ctx.client,
        ctx.request_id,
        code,
        schema_id,
        &result,
        Some(&serde_json::to_string(&result).expect("OrchestrationControlResult is serializable")),
    )
}

fn orchestration_control_err(
    ctx: OrchestrationCommandContext<'_>,
    code: ControlErrorCode,
    err: OrchestrationControlError,
) -> Vec<u8> {
    let wire_code = match err {
        OrchestrationControlError::NotFound(_) => ControlErrorCode::OrchNotFound,
        _ => code,
    };
    protocol::response_protocol_err_typed(
        ctx.client,
        ctx.request_id,
        wire_code,
        protocol::schema::ERROR,
        &err.to_string(),
    )
}
