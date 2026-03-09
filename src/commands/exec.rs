use crate::policy::resolve_exec_workload;
use crate::protocol;
use crate::scheduler::ProcessPriority;
use crate::services::model_runtime::activate_model_target;
use crate::services::process_runtime::spawn_managed_process;
use serde_json::json;

use super::context::CommandContext;
use super::metrics::log_event;

/// Handle the EXEC opcode: spawn a new inference process.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer (e.g. on error), or `Some(response)` on success.
pub(crate) fn handle_exec(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    let prompt_raw = String::from_utf8_lossy(payload).to_string();
    let (workload, hinted_workload, prompt) = resolve_exec_workload(&prompt_raw);
    let auto_switch = crate::config::kernel_config().exec.auto_switch;

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
                    match activate_model_target(ctx.engine_state, ctx.model_catalog, &target) {
                        Ok(loaded) => {
                            log_event(
                                "scheduler_model_switch",
                                ctx.client_id,
                                None,
                                &format!(
                                    "workload={:?} model_id={} family={:?} backend={}",
                                    workload,
                                    target.model_id.clone().unwrap_or_else(|| "<external-path>".to_string()),
                                    target.family,
                                    loaded.backend_id
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
        match spawn_managed_process(
            engine,
            ctx.memory,
            ctx.scheduler,
            &prompt,
            ctx.client_id,
            workload,
            ProcessPriority::Normal,
        ) {
            Ok(spawned) => {
                let pid = spawned.pid;

                ctx.metrics.inc_exec_started();
                log_event(
                    "process_spawn",
                    ctx.client_id,
                    Some(pid),
                    &format!("exec_started workload={:?} priority=normal", workload),
                );
                Some(protocol::response_ok_code(
                    "EXEC",
                    &json!({
                        "pid": pid,
                        "workload": format!("{:?}", workload).to_lowercase(),
                        "priority": "normal",
                    })
                    .to_string(),
                ))
            }
            Err(e) => Some(protocol::response_err_code("SPAWN_FAILED", &e)),
        }
    } else {
        Some(protocol::response_err_code("NO_MODEL", "No Model Loaded"))
    }
}
