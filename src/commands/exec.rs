use crate::policy::resolve_exec_policy;
use crate::protocol;
use crate::scheduler::ProcessPriority;
use crate::services::model_runtime::activate_model_target;
use crate::services::process_runtime::{spawn_managed_process, ManagedProcessRequest};
use agentic_control_models::{ExecStartPayload, KernelEvent};
use agentic_protocol::ControlErrorCode;

use super::context::ExecCommandContext;
use super::metrics::log_event;

/// Handle the EXEC opcode: spawn a new inference process.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer (e.g. on error), or `Some(response)` on success.
pub(crate) fn handle_exec(ctx: ExecCommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    let ExecCommandContext {
        client,
        request_id,
        memory,
        engine_state,
        model_catalog,
        scheduler,
        in_flight,
        client_id,
        pending_events,
        metrics,
    } = ctx;

    let prompt_raw = String::from_utf8_lossy(payload).to_string();
    let resolved = resolve_exec_policy(&prompt_raw);
    let workload = resolved.workload;
    let hinted_workload = resolved.hinted_workload;
    let prompt = resolved.prompt;
    let auto_switch = crate::config::kernel_config().exec.auto_switch;

    let _ = model_catalog.refresh();
    let can_scheduler_switch = auto_switch || hinted_workload.is_some();
    if can_scheduler_switch {
        match model_catalog.resolve_workload_target(workload) {
            Ok(Some(target)) => {
                let selected_path = target.path.to_string_lossy().to_string();
                let should_reload = match engine_state.as_ref() {
                    Some(engine) => {
                        engine.loaded_model_path() != selected_path
                            || engine.loaded_family() != target.family
                            || engine.loaded_backend_id()
                                != target.driver_resolution.resolved_backend_id
                    }
                    None => true,
                };
                if should_reload {
                    let live_processes = engine_state
                        .as_ref()
                        .map(|engine| engine.processes.len())
                        .unwrap_or(0);
                    if !in_flight.is_empty() || live_processes > 0 {
                        let response = protocol::response_protocol_err_typed(
                            client,
                            request_id,
                            ControlErrorCode::LoadBusy,
                            protocol::schema::ERROR,
                            &format!(
                                "Cannot switch model while {} process(es) are live and {} are in-flight",
                                live_processes,
                                in_flight.len()
                            ),
                        );
                        client.output_buffer.extend(response);
                        return None;
                    }
                    match activate_model_target(engine_state, model_catalog, &target) {
                        Ok(loaded) => {
                            log_event(
                                "scheduler_model_switch",
                                client_id,
                                None,
                                &format!(
                                    "workload={:?} model_id={} family={:?} backend={}",
                                    workload,
                                    target
                                        .model_id
                                        .clone()
                                        .unwrap_or_else(|| "<external-path>".to_string()),
                                    target.family,
                                    loaded.backend_id
                                ),
                            );
                        }
                        Err(e) => {
                            let response = protocol::response_protocol_err_typed(
                                client,
                                request_id,
                                ControlErrorCode::SchedulerLoadFailed,
                                protocol::schema::ERROR,
                                &e.to_string(),
                            );
                            client.output_buffer.extend(response);
                            return None;
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                let response = protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::SchedulerTargetFailed,
                    protocol::schema::ERROR,
                    &e.to_string(),
                );
                client.output_buffer.extend(response);
                return None;
            }
        }
    }

    if let Some(engine) = engine_state.as_mut() {
        match spawn_managed_process(
            engine,
            memory,
            scheduler,
            ManagedProcessRequest {
                prompt: prompt.clone(),
                owner_id: client_id,
                workload,
                priority: ProcessPriority::Normal,
                context_policy: Some(resolved.context_policy.clone()),
            },
        ) {
            Ok(spawned) => {
                let pid = spawned.pid;

                metrics.inc_exec_started();
                pending_events.push(KernelEvent::SessionStarted {
                    pid,
                    workload: format!("{:?}", workload).to_lowercase(),
                    prompt: prompt.clone(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "exec_started".to_string(),
                });
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "exec_started".to_string(),
                });
                log_event(
                    "process_spawn",
                    client_id,
                    Some(pid),
                    &format!("exec_started workload={:?} priority=normal", workload),
                );
                let payload = ExecStartPayload {
                    pid,
                    workload: format!("{:?}", workload).to_lowercase(),
                    priority: "normal".to_string(),
                    context_strategy: resolved.context_policy.strategy.label().to_string(),
                    context_window_size: resolved.context_policy.window_size_tokens,
                };
                Some(protocol::response_protocol_ok(
                    client,
                    request_id,
                    "EXEC",
                    protocol::schema::EXEC,
                    &payload,
                    Some(
                        &serde_json::to_string(&payload).expect("ExecStartPayload is serializable"),
                    ),
                ))
            }
            Err(e) => Some(protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::SpawnFailed,
                protocol::schema::ERROR,
                &e,
            )),
        }
    } else {
        Some(protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        ))
    }
}
