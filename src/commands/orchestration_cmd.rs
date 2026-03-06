use crate::protocol;
use crate::scheduler::ProcessPriority;

use super::context::CommandContext;
use super::metrics::{inc_exec_started, log_event};

/// Handle the ORCHESTRATE opcode.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer, or `Some(response)` on success.
pub(crate) fn handle_orchestrate(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    {
        let lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
        if lock.is_none() {
            ctx.client.output_buffer.extend(
                protocol::response_err_code("NO_MODEL", "No Model Loaded — ORCHESTRATE requires a loaded engine"),
            );
            return None;
        }
    }

    let payload_text = String::from_utf8_lossy(payload);
    match serde_json::from_str::<crate::orchestrator::TaskGraphDef>(payload_text.trim()) {
        Ok(graph) => {
            let total_tasks = graph.tasks.len();
            match ctx.orchestrator.register(graph, ctx.client_id) {
                Ok((orch_id, spawn_requests)) => {
                    let mut spawned = 0usize;
                    let mut lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
                    let engine = lock.as_mut().expect("engine verified present above");

                    for req in spawn_requests {
                        match engine.spawn_process(&req.prompt, 0, req.owner_id) {
                            Ok(pid) => {
                                if let Some(token_slots) = engine.process_max_tokens(pid) {
                                    if let Err(e) = ctx.memory.borrow_mut().register_process(pid, token_slots) {
                                        engine.kill_process(pid);
                                        ctx.orchestrator.mark_spawn_failed(orch_id, &req.task_id, &e.to_string());
                                        continue;
                                    }
                                }
                                ctx.scheduler.register(pid, req.workload, ProcessPriority::Normal);
                                ctx.orchestrator.register_pid(pid, orch_id, &req.task_id);
                                inc_exec_started();
                                spawned += 1;
                            }
                            Err(e) => {
                                ctx.orchestrator.mark_spawn_failed(orch_id, &req.task_id, &e.to_string());
                            }
                        }
                    }

                    log_event("orchestrate", ctx.client_id, None,
                        &format!("orch_id={} total={} spawned={}", orch_id, total_tasks, spawned));
                    Some(protocol::response_ok_code("ORCHESTRATE",
                        &format!("orchestration_id={} total_tasks={} spawned={}", orch_id, total_tasks, spawned)))
                }
                Err(e) => Some(protocol::response_err_code("ORCHESTRATE_INVALID", &e)),
            }
        }
        Err(e) => Some(protocol::response_err_code("ORCHESTRATE_JSON", &format!("Invalid task graph JSON: {}", e))),
    }
}
