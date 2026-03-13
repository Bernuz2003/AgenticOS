use crate::protocol;
use crate::services::orchestration_runtime::{start_orchestration, OrchestrationStartError};
use agentic_control_models::OrchestrateResult;
use agentic_protocol::ControlErrorCode;

use super::context::OrchestrationCommandContext;
use super::metrics::log_event;

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
