use agentic_control_models::KernelEvent;
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
use crate::session::SessionRegistry;
use crate::storage::StorageService;

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
    owner_id: usize,
    graph: TaskGraphDef,
) -> Result<OrchestrationStartResult, OrchestrationStartError> {
    let total_tasks = graph.tasks.len();
    let (orch_id, spawn_requests) = orchestrator.register(graph, owner_id)?;

    let mut spawned = 0usize;
    for req in spawn_requests {
        let runtime_id = resolve_runtime_for_spawn_request(
            runtime_registry,
            resource_governor,
            storage,
            model_catalog,
            session_registry,
            &req,
        )?;
        let pid_floor = runtime_registry.next_pid_floor();
        let spawn_result = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                return Err(OrchestrationStartError::NoModelLoaded);
            };
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
                    owner_id: req.owner_id,
                    workload: req.workload,
                    required_backend_class: req.required_backend_class,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                    context_policy: Some(req.context_policy.clone()),
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
                orchestrator.register_pid(spawned_process.pid, orch_id, &req.task_id);
                pending_events.push(KernelEvent::SessionStarted {
                    session_id: spawned_process.session_id.clone(),
                    pid: spawned_process.pid,
                    workload: format!("{:?}", req.workload).to_lowercase(),
                    prompt: req.prompt.clone(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid: spawned_process.pid,
                    reason: "orchestrate_started".to_string(),
                });
                spawned += 1;
            }
            Err(err) => {
                orchestrator.mark_spawn_failed(orch_id, &req.task_id, &err);
            }
        }
    }

    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestrate_started".to_string(),
    });

    Ok(OrchestrationStartResult {
        orchestration_id: orch_id,
        total_tasks,
        spawned,
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
