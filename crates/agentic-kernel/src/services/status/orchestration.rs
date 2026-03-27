use std::collections::HashMap;

use agentic_control_models::{
    ArtifactListResponse, IpcMessageView, OrchArtifactRefView, OrchArtifactView,
    OrchStatusResponse, OrchSummaryResponse, OrchTaskAttemptView, OrchTaskEntry,
    OrchestrationListResponse,
};

use super::process::build_pid_status;
use super::view::StatusSnapshotDeps;

pub fn build_orchestration_status(
    deps: &StatusSnapshotDeps<'_>,
    orch_id: u64,
) -> Option<OrchStatusResponse> {
    let orch = deps.orchestrator.get(orch_id)?;
    let workflow_io = deps.storage.load_workflow_io(orch_id).ok()?;
    let ipc_messages = deps
        .storage
        .load_ipc_messages_for_orchestration(orch_id)
        .unwrap_or_default();
    let (pending, running, completed, failed, skipped) = orch.counts();
    let total = orch.tasks.len();
    let elapsed = orch.created_at.elapsed().as_secs_f64();
    let finished = orch.is_finished();
    let attempts_by_task = workflow_io.attempts.into_iter().fold(
        HashMap::<String, Vec<crate::storage::StoredWorkflowTaskAttempt>>::new(),
        |mut acc, attempt| {
            acc.entry(attempt.task_id.clone()).or_default().push(attempt);
            acc
        },
    );
    let artifacts_by_id = workflow_io
        .artifacts
        .iter()
        .cloned()
        .map(|artifact| (artifact.artifact_id.clone(), artifact))
        .collect::<HashMap<_, _>>();
    let artifacts_by_task = workflow_io.artifacts.into_iter().fold(
        HashMap::<String, Vec<crate::storage::StoredWorkflowArtifact>>::new(),
        |mut acc, artifact| {
            acc.entry(artifact.producer_task_id.clone())
                .or_default()
                .push(artifact);
            acc
        },
    );
    let inputs_by_attempt = workflow_io.inputs.into_iter().fold(
        HashMap::<(String, u32), Vec<crate::storage::StoredWorkflowArtifactInput>>::new(),
        |mut acc, input| {
            acc.entry((input.consumer_task_id.clone(), input.consumer_attempt))
                .or_default()
                .push(input);
            acc
        },
    );

    let tasks: Vec<OrchTaskEntry> = orch
        .topo_order
        .iter()
        .map(|task_id| {
            let status = &orch.status[task_id];
            let task_def = orch.tasks.get(task_id);
            let task_attempts = attempts_by_task.get(task_id).cloned().unwrap_or_default();
            let output_artifacts = artifacts_by_task.get(task_id).cloned().unwrap_or_default();
            let current_attempt = match status {
                crate::orchestrator::TaskStatus::Running { attempt, .. }
                | crate::orchestrator::TaskStatus::Completed { attempt }
                | crate::orchestrator::TaskStatus::Failed { attempt, .. } => Some(*attempt),
                crate::orchestrator::TaskStatus::Pending
                | crate::orchestrator::TaskStatus::Skipped => {
                    task_attempts.first().map(|attempt| attempt.attempt)
                }
            };
            let selected_attempt =
                current_attempt.or_else(|| task_attempts.first().map(|attempt| attempt.attempt));
            let input_artifacts = selected_attempt
                .and_then(|attempt| inputs_by_attempt.get(&(task_id.clone(), attempt)).cloned())
                .unwrap_or_default()
                .into_iter()
                .map(|input| {
                    let source = artifacts_by_id.get(&input.artifact_id);
                    OrchArtifactRefView {
                        artifact_id: input.artifact_id,
                        task: input.producer_task_id,
                        attempt: input.producer_attempt,
                        kind: source
                            .map(|artifact| artifact.kind.clone())
                            .unwrap_or_else(|| "task_result".to_string()),
                        label: source
                            .map(|artifact| artifact.label.clone())
                            .unwrap_or_else(|| "task artifact".to_string()),
                    }
                })
                .collect::<Vec<_>>();
            let artifact_views = output_artifacts
                .iter()
                .cloned()
                .map(|artifact| OrchArtifactView {
                    artifact_id: artifact.artifact_id,
                    task: artifact.producer_task_id,
                    attempt: artifact.producer_attempt,
                    kind: artifact.kind,
                    label: artifact.label,
                    mime_type: artifact.mime_type,
                    preview: artifact.preview,
                    content: artifact.content_text,
                    bytes: artifact.bytes,
                    created_at_ms: artifact.created_at_ms,
                })
                .collect::<Vec<_>>();
            let running_output = deps.orchestrator.running_output_for_task(orch_id, task_id);
            let (pid, error, context) = match status {
                crate::orchestrator::TaskStatus::Running { pid, .. } => (
                    Some(*pid),
                    None,
                    build_pid_status(deps, *pid).and_then(|response| response.context),
                ),
                crate::orchestrator::TaskStatus::Failed { error, .. } => {
                    (None, Some(error.clone()), None)
                }
                _ => (None, None, None),
            };
            let latest_output_preview = if let Some(running_output) = running_output {
                (!running_output.text.is_empty()).then_some(running_output.text.clone())
            } else {
                artifact_views.first().map(|artifact| artifact.preview.clone())
            };
            let latest_output_text = if let Some(running_output) = running_output {
                (!running_output.text.is_empty()).then_some(running_output.text.clone())
            } else {
                artifact_views.first().map(|artifact| artifact.content.clone())
            };
            let latest_output_truncated = running_output
                .map(|running_output| running_output.truncated)
                .unwrap_or_else(|| {
                    task_attempts
                        .first()
                        .map(|attempt| attempt.truncated)
                        .unwrap_or(false)
                });
            let attempts = task_attempts
                .into_iter()
                .map(|attempt| {
                    let running_preview = running_output
                        .filter(|running| running.attempt == attempt.attempt)
                        .map(|running| running.text.clone());
                    OrchTaskAttemptView {
                        attempt: attempt.attempt,
                        status: attempt.status,
                        session_id: attempt.session_id,
                        pid: attempt.pid,
                        error: attempt.error,
                        output_preview: running_preview
                            .clone()
                            .filter(|preview| !preview.is_empty())
                            .unwrap_or(attempt.output_preview),
                        output_chars: running_preview
                            .as_ref()
                            .map(|preview| preview.len())
                            .unwrap_or(attempt.output_chars),
                        truncated: running_output
                            .filter(|running| running.attempt == attempt.attempt)
                            .map(|running| running.truncated)
                            .unwrap_or(attempt.truncated),
                        started_at_ms: attempt.started_at_ms,
                        completed_at_ms: attempt.completed_at_ms,
                        primary_artifact_id: attempt.primary_artifact_id,
                        termination_reason: attempt.termination_reason,
                    }
                })
                .collect::<Vec<_>>();
            let termination_reason = attempts
                .iter()
                .find_map(|attempt| attempt.termination_reason.clone());
            OrchTaskEntry {
                task: task_id.clone(),
                role: task_def.and_then(|task| task.role.clone()),
                workload: task_def.and_then(|task| task.workload.clone()),
                backend_class: task_def.and_then(|task| {
                    task.backend_class
                        .map(|backend_class| backend_class.as_str().to_string())
                }),
                context_strategy: task_def.and_then(|task| task.context_strategy.clone()),
                deps: task_def.map(|task| task.deps.clone()).unwrap_or_default(),
                status: status.label().to_string(),
                current_attempt,
                pid,
                error,
                context,
                latest_output_preview,
                latest_output_text,
                latest_output_truncated,
                input_artifacts,
                output_artifacts: artifact_views,
                attempts,
                termination_reason,
            }
        })
        .collect();

    let truncations = tasks
        .iter()
        .flat_map(|task: &OrchTaskEntry| task.attempts.iter())
        .filter(|attempt| attempt.truncated)
        .count();
    let output_chars_stored = tasks
        .iter()
        .map(|task| {
            task.output_artifacts
                .iter()
                .map(|artifact| artifact.bytes)
                .sum::<usize>()
        })
        .sum::<usize>()
        + tasks
            .iter()
            .filter_map(|task| {
                if task.status == "running" {
                    task.latest_output_text.as_ref().map(|text| text.len())
                } else {
                    None
                }
            })
            .sum::<usize>();

    Some(OrchStatusResponse {
        orchestration_id: orch_id,
        total,
        completed,
        running,
        pending,
        failed,
        skipped,
        finished,
        elapsed_secs: elapsed,
        policy: format!("{:?}", orch.failure_policy),
        truncations,
        output_chars_stored,
        tasks,
        ipc_messages: ipc_messages
            .into_iter()
            .map(|message| IpcMessageView {
                message_id: message.message_id,
                orchestration_id: message.orchestration_id,
                sender_pid: message.sender_pid,
                sender_task: message.sender_task_id,
                sender_attempt: message.sender_attempt,
                receiver_pid: message.receiver_pid,
                receiver_task: message.receiver_task_id,
                receiver_attempt: message.receiver_attempt,
                receiver_role: message.receiver_role,
                message_type: message.message_type,
                channel: message.channel,
                payload_preview: message.payload_preview,
                payload_text: message.payload_text,
                status: message.status,
                created_at_ms: message.created_at_ms,
                delivered_at_ms: message.delivered_at_ms,
                consumed_at_ms: message.consumed_at_ms,
                failed_at_ms: message.failed_at_ms,
            })
            .collect(),
    })
}

pub fn build_orchestration_list(deps: &StatusSnapshotDeps<'_>) -> OrchestrationListResponse {
    OrchestrationListResponse {
        orchestrations: build_orchestration_summaries(deps),
    }
}

pub fn build_artifact_list(
    deps: &StatusSnapshotDeps<'_>,
    orch_id: u64,
    task_filter: Option<&str>,
) -> Option<ArtifactListResponse> {
    deps.orchestrator.get(orch_id)?;
    let workflow_io = deps.storage.load_workflow_io(orch_id).ok()?;
    let task_filter_owned = task_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let artifacts = workflow_io
        .artifacts
        .into_iter()
        .filter(|artifact| {
            task_filter_owned
                .as_deref()
                .is_none_or(|task| artifact.producer_task_id == task)
        })
        .map(|artifact| OrchArtifactView {
            artifact_id: artifact.artifact_id,
            task: artifact.producer_task_id,
            attempt: artifact.producer_attempt,
            kind: artifact.kind,
            label: artifact.label,
            mime_type: artifact.mime_type,
            preview: artifact.preview,
            content: artifact.content_text,
            bytes: artifact.bytes,
            created_at_ms: artifact.created_at_ms,
        })
        .collect();

    Some(ArtifactListResponse {
        orchestration_id: orch_id,
        task: task_filter_owned,
        artifacts,
    })
}

pub(super) fn build_orchestration_summaries(
    deps: &StatusSnapshotDeps<'_>,
) -> Vec<OrchSummaryResponse> {
    deps.orchestrator
        .all_ids()
        .into_iter()
        .filter_map(|orch_id| {
            let orch = deps.orchestrator.get(orch_id)?;
            let (pending, running, completed, failed, skipped) = orch.counts();
            Some(OrchSummaryResponse {
                orchestration_id: orch_id,
                total: orch.tasks.len(),
                completed,
                running,
                pending,
                failed,
                skipped,
                finished: orch.is_finished(),
                elapsed_secs: orch.created_at.elapsed().as_secs_f64(),
                policy: format!("{:?}", orch.failure_policy),
            })
        })
        .collect()
}
