use agentic_control_models::{KernelEvent, OrchestrationControlResult, RetryTaskResult};
use thiserror::Error;

use crate::errors::OrchestratorError;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::{Orchestrator, TaskGraphDef};
use crate::process::ProcessLifecyclePolicy;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::model_runtime::activate_model_target;
use crate::services::process_control::{request_process_kill_with_session, ProcessSignalResult};
use crate::session::SessionRegistry;
use crate::session::SessionRegistryError;
use crate::storage::{current_timestamp_ms, StorageService, WorkflowArtifactInputRef};
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};

use std::collections::HashSet;

use super::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};

pub struct OrchestrationStartResult {
    pub orchestration_id: u64,
    pub total_tasks: usize,
    pub spawned: usize,
}

#[derive(Debug, Error)]
pub enum OrchestrationStartError {
    #[error("No Model Loaded — ORCHESTRATE requires a loaded engine")]
    NoModelLoaded,

    #[error("{0}")]
    InvalidGraph(#[from] OrchestratorError),

    #[error("{0}")]
    RoutingFailed(String),
}

#[derive(Debug, Error)]
pub enum OrchestrationRetryError {
    #[error("{0}")]
    InvalidTask(#[from] OrchestratorError),

    #[error("{0}")]
    RoutingFailed(String),
}

#[derive(Debug, Error)]
pub enum OrchestrationControlError {
    #[error("orchestration {0} not found")]
    NotFound(u64),

    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    ControlFailed(String),
}

#[allow(clippy::too_many_arguments)]
pub fn start_orchestration(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
    owner_id: usize,
    graph: TaskGraphDef,
) -> Result<OrchestrationStartResult, OrchestrationStartError> {
    let total_tasks = graph.tasks.len();
    let (orch_id, spawn_requests) = orchestrator.register(graph, owner_id)?;
    let spawned = spawn_workflow_requests(
        runtime_registry,
        resource_governor,
        memory,
        model_catalog,
        scheduler,
        orchestrator,
        session_registry,
        storage,
        pending_events,
        tool_registry,
        spawn_requests,
        "orchestrate_started",
    )
    .map_err(|err| match err.as_str() {
        "no_model_loaded" => OrchestrationStartError::NoModelLoaded,
        _ => OrchestrationStartError::RoutingFailed(err),
    })?;

    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestrate_started".to_string(),
    });

    Ok(OrchestrationStartResult {
        orchestration_id: orch_id,
        total_tasks,
        spawned,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn retry_orchestration_task(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
    orch_id: u64,
    task_id: &str,
) -> Result<RetryTaskResult, OrchestrationRetryError> {
    let plan = orchestrator.retry_task(orch_id, task_id)?;
    let (spawn_requests, kill_pids) = orchestrator.advance_one(orch_id);
    if !kill_pids.is_empty() {
        return Err(OrchestrationRetryError::RoutingFailed(
            "retry produced unexpected running-task kills".to_string(),
        ));
    }
    let spawned = spawn_workflow_requests(
        runtime_registry,
        resource_governor,
        memory,
        model_catalog,
        scheduler,
        orchestrator,
        session_registry,
        storage,
        pending_events,
        tool_registry,
        spawn_requests,
        "orchestrator_retry",
    )
    .map_err(OrchestrationRetryError::RoutingFailed)?;

    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestrator_retry".to_string(),
    });

    Ok(RetryTaskResult {
        orchestration_id: orch_id,
        task: task_id.to_string(),
        reset_tasks: plan.reset_tasks,
        spawned,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn stop_orchestration(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    job_scheduler: &mut crate::services::job_scheduler::JobScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    orch_id: u64,
) -> Result<OrchestrationControlResult, OrchestrationControlError> {
    let Some(plan) = orchestrator.stop(orch_id) else {
        return Err(OrchestrationControlError::NotFound(orch_id));
    };

    for finalized in plan.finalized_attempts {
        storage
            .finalize_workflow_task_attempt(
                finalized.orch_id,
                &finalized.task_id,
                finalized.attempt,
                &finalized.status,
                finalized.error.as_deref(),
                finalized.termination_reason.as_deref(),
                &finalized.output_text,
                finalized.truncated,
                current_timestamp_ms(),
            )
            .map_err(|err| OrchestrationControlError::ControlFailed(err.to_string()))?;
    }

    for pid in plan.kill_pids {
        match request_process_kill_with_session(
            runtime_registry,
            memory,
            scheduler,
            session_registry,
            storage,
            in_flight,
            pending_kills,
            pid,
        ) {
            ProcessSignalResult::Deferred | ProcessSignalResult::Applied => {
                pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "orchestration_stopped".to_string(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "orchestration_stopped".to_string(),
                });
            }
            ProcessSignalResult::NotFound | ProcessSignalResult::NoModelLoaded => {}
        }
    }

    job_scheduler
        .complete_orchestration(storage, orch_id, "cancelled", Some("orchestration_stopped"))
        .map_err(OrchestrationControlError::ControlFailed)?;

    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestration_stopped".to_string(),
    });

    Ok(OrchestrationControlResult {
        orchestration_id: orch_id,
        status: "stopped".to_string(),
    })
}

pub fn delete_orchestration(
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    orch_id: u64,
) -> Result<OrchestrationControlResult, OrchestrationControlError> {
    let Some(orch) = orchestrator.get(orch_id) else {
        return Err(OrchestrationControlError::NotFound(orch_id));
    };
    if !orch.is_finished() {
        return Err(OrchestrationControlError::Invalid(format!(
            "Orchestration {} is still running; stop it before deleting it.",
            orch_id
        )));
    }

    let workflow_io = storage
        .load_workflow_io(orch_id)
        .map_err(|err| OrchestrationControlError::ControlFailed(err.to_string()))?;
    let session_ids = workflow_io
        .attempts
        .iter()
        .filter_map(|attempt| attempt.session_id.clone())
        .collect::<HashSet<_>>();

    storage
        .delete_workflow_io(orch_id)
        .map_err(|err| OrchestrationControlError::ControlFailed(err.to_string()))?;
    storage
        .delete_ipc_messages_for_orchestration(orch_id)
        .map_err(|err| OrchestrationControlError::ControlFailed(err.to_string()))?;

    for session_id in session_ids {
        match session_registry.delete_session(storage, &session_id) {
            Ok(()) => {}
            Err(SessionRegistryError::SessionNotFound(_)) => {
                storage
                    .delete_session(&session_id)
                    .map_err(|err| OrchestrationControlError::ControlFailed(err.to_string()))?;
            }
            Err(err) => {
                return Err(OrchestrationControlError::ControlFailed(err.to_string()));
            }
        }
    }

    orchestrator.remove(orch_id);
    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestration_deleted".to_string(),
    });

    Ok(OrchestrationControlResult {
        orchestration_id: orch_id,
        status: "deleted".to_string(),
    })
}

pub(crate) fn resolve_runtime_for_spawn_request(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    storage: &mut StorageService,
    model_catalog: &mut ModelCatalog,
    session_registry: &SessionRegistry,
    req: &crate::orchestrator::SpawnRequest,
) -> Result<String, OrchestrationStartError> {
    if let Some(required_backend_class) = req.required_backend_class {
        if let Some(current_runtime_id) = runtime_registry.current_runtime_id() {
            if runtime_registry
                .descriptor(current_runtime_id)
                .is_some_and(|descriptor| descriptor.backend_class == required_backend_class)
            {
                return Ok(current_runtime_id.to_string());
            }
        }

        if let Some(runtime_id) =
            runtime_registry.loaded_runtime_id_for_backend_class(required_backend_class)
        {
            return Ok(runtime_id);
        }

        let target = model_catalog
            .resolve_workload_target(req.workload)
            .map_err(|err| OrchestrationStartError::RoutingFailed(err.to_string()))?
            .filter(|target| target.driver_resolution().backend_class == required_backend_class)
            .ok_or_else(|| {
                OrchestrationStartError::RoutingFailed(format!(
                    "No runtime target available for workload '{:?}' and backend class '{}'.",
                    req.workload,
                    required_backend_class.as_str()
                ))
            })?;
        let loaded = activate_model_target(
            runtime_registry,
            resource_governor,
            session_registry,
            storage,
            model_catalog,
            &target,
        )
        .map_err(|err| OrchestrationStartError::RoutingFailed(err.message().to_string()))?;
        return Ok(loaded.runtime_id);
    }

    if let Some(current_runtime_id) = runtime_registry.current_runtime_id() {
        return Ok(current_runtime_id.to_string());
    }

    if let Some(target) = model_catalog
        .resolve_workload_target(req.workload)
        .map_err(|err| OrchestrationStartError::RoutingFailed(err.to_string()))?
    {
        let loaded = activate_model_target(
            runtime_registry,
            resource_governor,
            session_registry,
            storage,
            model_catalog,
            &target,
        )
        .map_err(|err| OrchestrationStartError::RoutingFailed(err.message().to_string()))?;
        return Ok(loaded.runtime_id);
    }

    if let Some(runtime_id) = runtime_registry.any_loaded_runtime_id() {
        return Ok(runtime_id);
    }

    match model_catalog.resolve_load_target("") {
        Ok(target) => activate_model_target(
            runtime_registry,
            resource_governor,
            session_registry,
            storage,
            model_catalog,
            &target,
        )
        .map(|loaded| loaded.runtime_id)
        .map_err(|err| OrchestrationStartError::RoutingFailed(err.message().to_string())),
        Err(_) => Err(OrchestrationStartError::NoModelLoaded),
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_workflow_requests(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
    spawn_requests: Vec<crate::orchestrator::SpawnRequest>,
    event_reason: &str,
) -> Result<usize, String> {
    let system_prompt =
        crate::agent_prompt::build_agent_system_prompt(tool_registry, ToolCaller::AgentSupervisor);
    let mut spawned = 0usize;

    for req in spawn_requests {
        let permission_policy = ProcessPermissionPolicy::workflow_supervisor(
            tool_registry,
            Some(&req.permission_overrides),
        )
        .map_err(|err| err.to_string())?;
        let runtime_id = resolve_runtime_for_spawn_request(
            runtime_registry,
            resource_governor,
            storage,
            model_catalog,
            session_registry,
            &req,
        )
        .map_err(|err| match err {
            OrchestrationStartError::NoModelLoaded => "no_model_loaded".to_string(),
            other => other.to_string(),
        })?;
        let pid_floor = runtime_registry.next_pid_floor();
        let spawn_result = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                return Err("no_model_loaded".to_string());
            };
            let effective_context_policy = req
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
                    prompt: req.prompt.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    owner_id: req.owner_id,
                    tool_caller: ToolCaller::AgentSupervisor,
                    permission_policy: Some(permission_policy),
                    workload: req.workload,
                    required_backend_class: req.required_backend_class,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                    context_policy: Some(effective_context_policy),
                    quota_override: None,
                },
            )
        };

        match spawn_result {
            Ok(spawned_process) => {
                if let Err(err) =
                    runtime_registry.register_pid(storage, &runtime_id, spawned_process.pid)
                {
                    tracing::warn!(
                        pid = spawned_process.pid,
                        runtime_id,
                        %err,
                        "ORCHESTRATION: failed to register pid in runtime registry"
                    );
                }
                storage
                    .begin_workflow_task_attempt(
                        req.orch_id,
                        &req.task_id,
                        req.attempt,
                        Some(&spawned_process.session_id),
                        Some(spawned_process.pid),
                        current_timestamp_ms(),
                        &req.input_artifacts
                            .iter()
                            .map(|artifact| WorkflowArtifactInputRef {
                                artifact_id: artifact.artifact_id.clone(),
                                producer_task_id: artifact.producer_task_id.clone(),
                                producer_attempt: artifact.producer_attempt,
                            })
                            .collect::<Vec<_>>(),
                    )
                    .map_err(|err| err.to_string())?;
                orchestrator.register_pid(
                    spawned_process.pid,
                    req.orch_id,
                    &req.task_id,
                    req.attempt,
                );
                pending_events.push(KernelEvent::SessionStarted {
                    session_id: spawned_process.session_id.clone(),
                    pid: spawned_process.pid,
                    workload: format!("{:?}", req.workload).to_lowercase(),
                    prompt: req.prompt.clone(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid: spawned_process.pid,
                    reason: event_reason.to_string(),
                });
                spawned += 1;
            }
            Err(err) => {
                storage
                    .record_workflow_task_spawn_failure(
                        req.orch_id,
                        &req.task_id,
                        req.attempt,
                        &err,
                        current_timestamp_ms(),
                    )
                    .map_err(|storage_err| storage_err.to_string())?;
                let _ =
                    orchestrator.mark_spawn_failed(req.orch_id, &req.task_id, req.attempt, &err);
            }
        }
    }

    Ok(spawned)
}
