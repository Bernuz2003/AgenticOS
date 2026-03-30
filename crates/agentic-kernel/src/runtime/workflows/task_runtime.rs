use agentic_control_models::KernelEvent;

use crate::orchestrator::{Orchestrator, SpawnRequest};
use crate::services::process_runtime::ManagedProcessSpawn;
use crate::storage::{current_timestamp_ms, StorageService, WorkflowArtifactInputRef};

pub(super) fn record_spawn_failure(
    storage: &mut StorageService,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
    orch_id: u64,
    task_id: &str,
    attempt: u32,
    error: &str,
) {
    if let Err(storage_err) = storage.record_workflow_task_spawn_failure(
        orch_id,
        task_id,
        attempt,
        error,
        current_timestamp_ms(),
    ) {
        tracing::warn!(
            orch_id,
            task_id = %task_id,
            attempt,
            %storage_err,
            "ORCHESTRATOR: failed to persist spawn failure"
        );
    }
    let _ = orchestrator.mark_spawn_failed(orch_id, task_id, attempt, error);
    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestrator_spawn_failed".to_string(),
    });
}

pub(super) fn record_spawn_start(
    storage: &mut StorageService,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
    req: &SpawnRequest,
    spawned_process: &ManagedProcessSpawn,
) {
    if let Err(err) = storage.begin_workflow_task_attempt(
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
    ) {
        tracing::warn!(
            orch_id = req.orch_id,
            task_id = %req.task_id,
            attempt = req.attempt,
            %err,
            "ORCHESTRATOR: failed to persist started task attempt"
        );
    }
    orchestrator.register_pid(spawned_process.pid, req.orch_id, &req.task_id, req.attempt);
    pending_events.push(KernelEvent::SessionStarted {
        session_id: spawned_process.session_id.clone(),
        pid: spawned_process.pid,
        workload: format!("{:?}", req.workload).to_lowercase(),
        prompt: req.prompt.clone(),
    });
    pending_events.push(KernelEvent::WorkspaceChanged {
        pid: spawned_process.pid,
        reason: "orchestrator_spawned".to_string(),
    });
    pending_events.push(KernelEvent::LobbyChanged {
        reason: "orchestrator_spawned".to_string(),
    });
}
