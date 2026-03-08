use crate::config::env_bool;
use crate::engine::LLMEngine;
use crate::model_catalog::{infer_workload_class, parse_workload_hint};
use crate::protocol;
use crate::scheduler::ProcessPriority;

use super::context::CommandContext;
use super::metrics::log_event;

/// Handle the EXEC opcode: spawn a new inference process.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer (e.g. on error), or `Some(response)` on success.
pub(crate) fn handle_exec(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    let prompt_raw = String::from_utf8_lossy(payload).to_string();
    let (hinted_workload, prompt) = parse_workload_hint(&prompt_raw);
    let workload = hinted_workload.unwrap_or_else(|| infer_workload_class(&prompt));
    let auto_switch = env_bool("AGENTIC_EXEC_AUTO_SWITCH", false);

    let _ = ctx.model_catalog.refresh();
    let can_scheduler_switch = auto_switch || hinted_workload.is_some();
    if can_scheduler_switch {
        match ctx.model_catalog.resolve_workload_target(workload) {
            Ok(Some(target)) => {
                let selected_path = target.path.to_string_lossy().to_string();
                let should_reload = match ctx.engine_state.as_ref() {
                    Some(engine) => {
                    engine.loaded_model_path() != selected_path
                            || engine.loaded_family() != target.family
                            || engine.loaded_backend_id() != target.driver_resolution.resolved_backend_id
                    }
                    None => true,
                };
                if should_reload {
                    if !ctx.in_flight.is_empty() {
                        ctx.client.output_buffer.extend(protocol::response_err_code(
                            "IN_FLIGHT",
                            &format!(
                                "Cannot switch model while {} process(es) are in-flight",
                                ctx.in_flight.len()
                            ),
                        ));
                        return None;
                    }
                    *ctx.engine_state = None;
                    match LLMEngine::load_target(&target) {
                        Ok(new_engine) => {
                            let loaded_backend = new_engine.loaded_backend_id().to_string();
                            *ctx.engine_state = Some(new_engine);
                            if let Some(model_id) = target.model_id.as_ref() {
                                ctx.model_catalog.selected_id = Some(model_id.clone());
                            }
                            log_event(
                                "scheduler_model_switch",
                                ctx.client_id,
                                None,
                                &format!(
                                    "workload={:?} model_id={} family={:?} backend={}",
                                    workload,
                                    target.model_id.clone().unwrap_or_else(|| "<external-path>".to_string()),
                                    target.family,
                                    loaded_backend
                                ),
                            );
                        }
                        Err(e) => {
                            ctx.client.output_buffer.extend(protocol::response_err_code(
                                "SCHEDULER_LOAD_FAILED",
                                &format!("{}", e),
                            ));
                            return None;
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                ctx.client.output_buffer.extend(protocol::response_err_code(
                    "SCHEDULER_TARGET_FAILED",
                    &e.to_string(),
                ));
                return None;
            }
        }
    }

    if let Some(engine) = ctx.engine_state.as_mut() {
        match engine.spawn_process(&prompt, 0, ctx.client_id) {
            Ok(pid) => {
                if let Some(token_slots) = engine.process_max_tokens(pid) {
                    if let Err(e) = ctx.memory.register_process(pid, token_slots) {
                        engine.kill_process(pid);
                        ctx.client.output_buffer.extend(protocol::response_err_code(
                            "MEMORY_ADMISSION",
                            &e.to_string(),
                        ));
                        return None;
                    }
                }

                ctx.scheduler
                    .register(pid, workload, ProcessPriority::Normal);

                ctx.metrics.inc_exec_started();
                log_event(
                    "process_spawn",
                    ctx.client_id,
                    Some(pid),
                    &format!("exec_started workload={:?} priority=normal", workload),
                );
                Some(protocol::response_ok(&format!(
                    "Process Started PID: {} workload={:?} priority=normal",
                    pid, workload
                )))
            }
            Err(e) => Some(protocol::response_err_code("SPAWN_FAILED", &format!("{}", e))),
        }
    } else {
        Some(protocol::response_err_code("NO_MODEL", "No Model Loaded"))
    }
}
