use crate::policy::resolve_exec_policy;
use crate::process::ProcessLifecyclePolicy;
use crate::protocol;
use crate::scheduler::ProcessPriority;
use crate::services::model_runtime::{activate_model_target, ModelActivationError};
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use agentic_control_models::{ExecStartPayload, KernelEvent};
use agentic_protocol::ControlErrorCode;

use super::context::ExecCommandContext;
use super::diagnostics::log_event;

/// Handle the EXEC opcode: spawn a new inference process.
///
/// Returns `None` if an early-return response was written directly to the
/// client output buffer (e.g. on error), or `Some(response)` on success.
pub(crate) fn handle_exec(ctx: ExecCommandContext<'_>, payload: &[u8]) -> Option<Vec<u8>> {
    let ExecCommandContext {
        client,
        request_id,
        memory,
        runtime_registry,
        resource_governor,
        model_catalog,
        scheduler,
        in_flight: _in_flight,
        client_id,
        pending_events,
        metrics,
        session_registry,
        storage,
        tool_registry,
    } = ctx;

    let prompt_raw = String::from_utf8_lossy(payload).to_string();
    let resolved = resolve_exec_policy(&prompt_raw);
    let workload = resolved.workload;
    let hinted_workload = resolved.hinted_workload;
    let prompt = resolved.prompt;
    let system_prompt =
        crate::agent_prompt::build_agent_system_prompt(tool_registry, ToolCaller::AgentText);
    let auto_switch = crate::config::kernel_config().exec.auto_switch;

    let _ = model_catalog.refresh();
    let can_scheduler_switch = auto_switch || hinted_workload.is_some();
    let mut runtime_id = runtime_registry
        .current_runtime_id()
        .map(ToString::to_string);
    if can_scheduler_switch {
        match model_catalog.resolve_workload_target(workload) {
            Ok(Some(target)) => {
                match activate_model_target(
                    runtime_registry,
                    resource_governor,
                    session_registry,
                    storage,
                    model_catalog,
                    &target,
                ) {
                    Ok(loaded) => {
                        runtime_id = Some(loaded.runtime_id.clone());
                        log_event(
                            "scheduler_model_switch",
                            client_id,
                            None,
                            &format!(
                                "workload={:?} runtime_id={} model_id={} family={:?} backend={}",
                                workload,
                                loaded.runtime_id,
                                target
                                    .local_model_id()
                                    .or_else(|| target.remote_model_id())
                                    .unwrap_or("<external-path>"),
                                target.family(),
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
                            e.message(),
                        );
                        client.output_buffer.extend(response);
                        return None;
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

    if runtime_id.is_none() && runtime_registry.current_engine().is_none() {
        if let Ok(target) = model_catalog.resolve_load_target("") {
            match activate_model_target(
                runtime_registry,
                resource_governor,
                session_registry,
                storage,
                model_catalog,
                &target,
            ) {
                Ok(loaded) => {
                    runtime_id = Some(loaded.runtime_id);
                }
                Err(ModelActivationError::Busy(e)) => {
                    return Some(protocol::response_protocol_err_typed(
                        client,
                        request_id,
                        ControlErrorCode::LoadBusy,
                        protocol::schema::ERROR,
                        &e,
                    ));
                }
                Err(ModelActivationError::Failed(e)) => {
                    return Some(protocol::response_protocol_err_typed(
                        client,
                        request_id,
                        ControlErrorCode::LoadFailed,
                        protocol::schema::ERROR,
                        &e,
                    ));
                }
            }
        }
    }

    let runtime_id = runtime_id.or_else(|| {
        runtime_registry
            .current_runtime_id()
            .map(ToString::to_string)
    });

    if let Some(runtime_id) = runtime_id {
        let permission_policy = match ProcessPermissionPolicy::interactive_chat(tool_registry) {
            Ok(policy) => policy,
            Err(err) => {
                return Some(protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::SpawnFailed,
                    protocol::schema::ERROR,
                    &err,
                ));
            }
        };
        let pid_floor = runtime_registry.next_pid_floor();
        let spawn_result = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                return Some(protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::NoModel,
                    protocol::schema::ERROR,
                    "No Model Loaded",
                ));
            };
            let effective_context_policy = resolved
                .context_policy
                .align_to_runtime_window_if_default(engine.effective_context_window_tokens());
            spawn_managed_process_with_session(
                &runtime_id,
                pid_floor,
                engine,
                memory,
                scheduler,
                session_registry,
                storage,
                ManagedProcessRequest {
                    prompt: prompt.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    owner_id: client_id,
                    tool_caller: ToolCaller::AgentText,
                    permission_policy: Some(permission_policy.clone()),
                    workload,
                    required_backend_class: None,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                    context_policy: Some(effective_context_policy.clone()),
                },
            )
            .map(|spawned| (spawned, effective_context_policy))
        };

        match spawn_result {
            Ok((spawned, effective_context_policy)) => {
                let session_id = spawned.session_id.clone();
                let pid = spawned.pid;
                if let Err(err) = runtime_registry.register_pid(storage, &runtime_id, pid) {
                    tracing::warn!(
                        pid,
                        runtime_id,
                        %err,
                        "EXEC: failed to register pid in runtime registry"
                    );
                }

                metrics.inc_exec_started();
                pending_events.push(KernelEvent::SessionStarted {
                    session_id: session_id.clone(),
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
                    session_id,
                    pid,
                    workload: format!("{:?}", workload).to_lowercase(),
                    priority: "normal".to_string(),
                    context_strategy: effective_context_policy.strategy.label().to_string(),
                    context_window_size: effective_context_policy.window_size_tokens,
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
