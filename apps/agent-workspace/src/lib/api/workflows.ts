import { invoke } from "@tauri-apps/api/core";

import type {
  LobbyOrchestrationSummary,
  OrchestrationArtifact,
  OrchestrationStatus,
  OrchestrateResult,
  RetryWorkflowTaskResult,
  WorkflowRunControlResult,
} from "./index";
import { formatElapsedLabel } from "./normalizers";

export async function orchestrate(payload: string): Promise<OrchestrateResult> {
  const result = await invoke<{
    orchestration_id: number;
    total_tasks: number;
    spawned: number;
  }>("orchestrate", { payload });

  return {
    orchestrationId: result.orchestration_id,
    totalTasks: result.total_tasks,
    spawned: result.spawned,
  };
}

export async function listOrchestrations(): Promise<LobbyOrchestrationSummary[]> {
  const result = await invoke<{
    orchestrations: Array<{
      orchestration_id: number;
      total: number;
      completed: number;
      running: number;
      pending: number;
      failed: number;
      skipped: number;
      finished: boolean;
      elapsed_secs: number;
      policy: string;
    }>;
  }>("list_orchestrations");

  return result.orchestrations.map((orchestration) => ({
    orchestrationId: orchestration.orchestration_id,
    total: orchestration.total,
    completed: orchestration.completed,
    running: orchestration.running,
    pending: orchestration.pending,
    failed: orchestration.failed,
    skipped: orchestration.skipped,
    finished: orchestration.finished,
    elapsedLabel: formatElapsedLabel(orchestration.elapsed_secs),
    policy: orchestration.policy,
  }));
}

export async function listWorkflowArtifacts(
  orchestrationId: number,
  task: string | null = null,
): Promise<OrchestrationArtifact[]> {
  const result = await invoke<{
    artifacts: Array<{
      artifact_id: string;
      task: string;
      attempt: number;
      kind: string;
      label: string;
      mime_type: string;
      preview: string;
      content: string;
      bytes: number;
      created_at_ms: number;
    }>;
  }>("list_workflow_artifacts", {
    orchestrationId,
    task,
  });

  return result.artifacts.map((artifact) => ({
    artifactId: artifact.artifact_id,
    task: artifact.task,
    attempt: artifact.attempt,
    kind: artifact.kind,
    label: artifact.label,
    mimeType: artifact.mime_type,
    preview: artifact.preview,
    content: artifact.content,
    bytes: artifact.bytes,
    createdAtMs: artifact.created_at_ms,
  }));
}

export async function fetchOrchestrationStatus(
  orchestrationId: number,
): Promise<OrchestrationStatus> {
  const result = await invoke<any>("fetch_orchestration_status", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    total: result.total,
    completed: result.completed,
    running: result.running,
    pending: result.pending,
    failed: result.failed,
    skipped: result.skipped,
    finished: result.finished,
    elapsedSecs: result.elapsed_secs,
    policy: result.policy,
    truncations: result.truncations,
    outputCharsStored: result.output_chars_stored,
    tasks: result.tasks.map((task: any) => ({
      task: task.task,
      role: task.role ?? null,
      workload: task.workload ?? null,
      backendClass: task.backend_class ?? null,
      contextStrategy: task.context_strategy ?? null,
      deps: task.deps ?? [],
      status: task.status,
      currentAttempt: task.current_attempt ?? null,
      pid: task.pid,
      error: task.error,
      context: task.context
        ? {
            contextStrategy: task.context.context_strategy,
            contextTokensUsed: task.context.context_tokens_used,
            contextWindowSize: task.context.context_window_size,
            contextCompressions: task.context.context_compressions,
            contextRetrievalHits: task.context.context_retrieval_hits,
            contextRetrievalRequests: task.context.context_retrieval_requests,
            contextRetrievalMisses: task.context.context_retrieval_misses,
            contextRetrievalCandidatesScored:
              task.context.context_retrieval_candidates_scored,
            contextRetrievalSegmentsSelected:
              task.context.context_retrieval_segments_selected,
            lastRetrievalCandidatesScored:
              task.context.last_retrieval_candidates_scored,
            lastRetrievalSegmentsSelected:
              task.context.last_retrieval_segments_selected,
            lastRetrievalLatencyMs: task.context.last_retrieval_latency_ms,
            lastRetrievalTopScore: task.context.last_retrieval_top_score ?? null,
            lastCompactionReason: task.context.last_compaction_reason,
            lastSummaryTs: task.context.last_summary_ts,
            contextSegments: task.context.context_segments,
            episodicSegments: task.context.episodic_segments,
            episodicTokens: task.context.episodic_tokens,
            retrieveTopK: task.context.retrieve_top_k,
            retrieveCandidateLimit: task.context.retrieve_candidate_limit,
            retrieveMaxSegmentChars: task.context.retrieve_max_segment_chars,
            retrieveMinScore: task.context.retrieve_min_score,
          }
        : null,
      latestOutputPreview: task.latest_output_preview ?? null,
      latestOutputText: task.latest_output_text ?? null,
      latestOutputTruncated: task.latest_output_truncated ?? false,
      inputArtifacts: (task.input_artifacts ?? []).map((artifact: any) => ({
        artifactId: artifact.artifact_id,
        task: artifact.task,
        attempt: artifact.attempt,
        kind: artifact.kind,
        label: artifact.label,
      })),
      outputArtifacts: (task.output_artifacts ?? []).map((artifact: any) => ({
        artifactId: artifact.artifact_id,
        task: artifact.task,
        attempt: artifact.attempt,
        kind: artifact.kind,
        label: artifact.label,
        mimeType: artifact.mime_type,
        preview: artifact.preview,
        content: artifact.content,
        bytes: artifact.bytes,
        createdAtMs: artifact.created_at_ms,
      })),
      attempts: (task.attempts ?? []).map((attempt: any) => ({
        attempt: attempt.attempt,
        status: attempt.status,
        sessionId: attempt.session_id ?? null,
        pid: attempt.pid ?? null,
        error: attempt.error ?? null,
        outputPreview: attempt.output_preview,
        outputChars: attempt.output_chars,
        truncated: attempt.truncated,
        startedAtMs: attempt.started_at_ms,
        completedAtMs: attempt.completed_at_ms ?? null,
        primaryArtifactId: attempt.primary_artifact_id ?? null,
        terminationReason: attempt.termination_reason ?? null,
      })),
      terminationReason: task.termination_reason ?? null,
    })),
    ipcMessages: (result.ipc_messages ?? []).map((message: any) => ({
      messageId: message.message_id,
      orchestrationId: message.orchestration_id ?? null,
      senderPid: message.sender_pid ?? null,
      senderTask: message.sender_task ?? null,
      senderAttempt: message.sender_attempt ?? null,
      receiverPid: message.receiver_pid ?? null,
      receiverTask: message.receiver_task ?? null,
      receiverAttempt: message.receiver_attempt ?? null,
      receiverRole: message.receiver_role ?? null,
      messageType: message.message_type,
      channel: message.channel ?? null,
      payloadPreview: message.payload_preview,
      payloadText: message.payload_text,
      status: message.status,
      createdAtMs: message.created_at_ms,
      deliveredAtMs: message.delivered_at_ms ?? null,
      consumedAtMs: message.consumed_at_ms ?? null,
      failedAtMs: message.failed_at_ms ?? null,
    })),
  };
}

export async function retryWorkflowTask(
  orchestrationId: number,
  taskId: string,
): Promise<RetryWorkflowTaskResult> {
  const result = await invoke<{
    orchestration_id: number;
    task: string;
    reset_tasks: string[];
    spawned: number;
  }>("retry_workflow_task", { orchestrationId, taskId });

  return {
    orchestrationId: result.orchestration_id,
    task: result.task,
    resetTasks: result.reset_tasks,
    spawned: result.spawned,
  };
}

export async function stopWorkflowRun(
  orchestrationId: number,
): Promise<WorkflowRunControlResult> {
  const result = await invoke<{
    orchestration_id: number;
    status: string;
  }>("stop_workflow_run", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    status: result.status,
  };
}

export async function deleteWorkflowRun(
  orchestrationId: number,
): Promise<WorkflowRunControlResult> {
  const result = await invoke<{
    orchestration_id: number;
    status: string;
  }>("delete_workflow_run", { orchestrationId });

  return {
    orchestrationId: result.orchestration_id,
    status: result.status,
  };
}
