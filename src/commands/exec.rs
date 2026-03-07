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
        if let Some(selected) = ctx.model_catalog.select_for_workload(workload).cloned() {
            let should_reload = *ctx.active_family != selected.family;
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
                let tokenizer_hint = selected.tokenizer_path.clone();
                match LLMEngine::load(
                    selected.path.to_string_lossy().as_ref(),
                    selected.family,
                    tokenizer_hint,
                ) {
                    Ok(new_engine) => {
                        *ctx.engine_state = Some(new_engine);
                        *ctx.active_family = selected.family;
                        ctx.model_catalog.selected_id = Some(selected.id.clone());
                        log_event(
                            "scheduler_model_switch",
                            ctx.client_id,
                            None,
                            &format!(
                                "workload={:?} model_id={} family={:?}",
                                workload, selected.id, selected.family
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
