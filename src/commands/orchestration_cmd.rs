use crate::protocol;
use crate::scheduler::ProcessPriority;
use crate::services::process_runtime::spawn_managed_process;

use super::context::CommandContext;
use super::metrics::log_event;

/// Handle the ORCHESTRATE opcode.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer, or `Some(response)` on success.
pub(crate) fn handle_orchestrate(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    if ctx.engine_state.is_none() {
        ctx.client.output_buffer.extend(
            protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "NO_MODEL",
                protocol::schema::ERROR,
                "No Model Loaded — ORCHESTRATE requires a loaded engine",
            ),
        );
        return None;
    }

    let payload_text = String::from_utf8_lossy(payload);
    match serde_json::from_str::<crate::orchestrator::TaskGraphDef>(payload_text.trim()) {
        Ok(graph) => {
            let total_tasks = graph.tasks.len();
            match ctx.orchestrator.register(graph, ctx.client_id) {
                Ok((orch_id, spawn_requests)) => {
                    let mut spawned = 0usize;
                    let Some(engine) = ctx.engine_state.as_mut() else {
                        return Some(protocol::response_protocol_err(
                            ctx.client,
                            &ctx.request_id,
                            "NO_MODEL",
                            protocol::schema::ERROR,
                            "No Model Loaded — ORCHESTRATE requires a loaded engine",
                        ));
                    };

                    for req in spawn_requests {
                        match spawn_managed_process(
                            engine,
                            ctx.memory,
                            ctx.scheduler,
                            &req.prompt,
                            req.owner_id,
                            req.workload,
                            ProcessPriority::Normal,
                        ) {
                            Ok(spawned_process) => {
                                ctx.orchestrator.register_pid(spawned_process.pid, orch_id, &req.task_id);
                                ctx.metrics.inc_exec_started();
                                spawned += 1;
                            }
                            Err(e) => {
                                ctx.orchestrator.mark_spawn_failed(orch_id, &req.task_id, &e);
                            }
                        }
                    }

                    log_event("orchestrate", ctx.client_id, None,
                        &format!("orch_id={} total={} spawned={}", orch_id, total_tasks, spawned));
                    let json = serde_json::json!({
                        "orchestration_id": orch_id,
                        "total_tasks": total_tasks,
                        "spawned": spawned,
                    });
                    Some(protocol::response_protocol_ok(
                        ctx.client,
                        &ctx.request_id,
                        "ORCHESTRATE",
                        protocol::schema::ORCHESTRATE,
                        &json,
                        Some(&json.to_string()),
                    ))
                }
                Err(e) => Some(protocol::response_protocol_err(
                    ctx.client,
                    &ctx.request_id,
                    "ORCHESTRATE_INVALID",
                    protocol::schema::ERROR,
                    &e.to_string(),
                )),
            }
        }
        Err(e) => Some(protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "ORCHESTRATE_JSON",
            protocol::schema::ERROR,
            &format!("Invalid task graph JSON: {}", e),
        )),
    }
}
