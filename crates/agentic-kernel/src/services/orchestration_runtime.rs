use agentic_control_models::KernelEvent;
use thiserror::Error;

use crate::engine::LLMEngine;
use crate::errors::OrchestratorError;
use crate::memory::NeuralMemory;
use crate::orchestrator::{Orchestrator, TaskGraphDef};
use crate::process::ProcessLifecyclePolicy;
use crate::scheduler::{ProcessPriority, ProcessScheduler};

use super::process_runtime::{spawn_managed_process, ManagedProcessRequest};

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
}

pub fn start_orchestration(
    engine_state: &mut Option<LLMEngine>,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
    owner_id: usize,
    graph: TaskGraphDef,
) -> Result<OrchestrationStartResult, OrchestrationStartError> {
    let total_tasks = graph.tasks.len();
    let (orch_id, spawn_requests) = orchestrator.register(graph, owner_id)?;

    let Some(engine) = engine_state.as_mut() else {
        return Err(OrchestrationStartError::NoModelLoaded);
    };

    let mut spawned = 0usize;
    for req in spawn_requests {
        match spawn_managed_process(
            engine,
            memory,
            scheduler,
            ManagedProcessRequest {
                prompt: req.prompt.clone(),
                owner_id: req.owner_id,
                workload: req.workload,
                required_backend_class: req.required_backend_class,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                context_policy: Some(req.context_policy.clone()),
            },
        ) {
            Ok(spawned_process) => {
                orchestrator.register_pid(spawned_process.pid, orch_id, &req.task_id);
                pending_events.push(KernelEvent::SessionStarted {
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
