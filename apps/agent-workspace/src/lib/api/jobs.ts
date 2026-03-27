import { invoke } from "@tauri-apps/api/core";

import type {
  ScheduleJobResult,
  ScheduledJob,
  ScheduledJobControlResult,
} from "./index";

export async function scheduleWorkflowJob(
  payload: string,
): Promise<ScheduleJobResult> {
  const result = await invoke<{
    job_id: number;
    next_run_at_ms: number | null;
    trigger_kind: string;
  }>("schedule_workflow_job", { payload });

  return {
    jobId: result.job_id,
    nextRunAtMs: result.next_run_at_ms,
    triggerKind: result.trigger_kind,
  };
}

export async function listScheduledJobs(): Promise<ScheduledJob[]> {
  const result = await invoke<{ jobs: any[] }>("list_scheduled_jobs");

  return result.jobs.map((job) => ({
    jobId: job.job_id,
    name: job.name,
    targetKind: job.target_kind,
    triggerKind: job.trigger_kind,
    triggerLabel: job.trigger_label,
    enabled: job.enabled,
    state: job.state,
    nextRunAtMs: job.next_run_at_ms,
    currentTriggerAtMs: job.current_trigger_at_ms,
    currentAttempt: job.current_attempt,
    timeoutMs: job.timeout_ms,
    maxRetries: job.max_retries,
    backoffMs: job.backoff_ms,
    lastRunStartedAtMs: job.last_run_started_at_ms,
    lastRunCompletedAtMs: job.last_run_completed_at_ms,
    lastRunStatus: job.last_run_status,
    lastError: job.last_error,
    consecutiveFailures: job.consecutive_failures,
    activeOrchestrationId: job.active_orchestration_id,
    recentRuns: job.recent_runs.map((run: any) => ({
      runId: run.run_id,
      triggerAtMs: run.trigger_at_ms,
      attempt: run.attempt,
      status: run.status,
      startedAtMs: run.started_at_ms,
      completedAtMs: run.completed_at_ms,
      orchestrationId: run.orchestration_id,
      deadlineAtMs: run.deadline_at_ms,
      error: run.error,
    })),
  }));
}

export async function setScheduledJobEnabled(
  jobId: number,
  enabled: boolean,
): Promise<ScheduledJobControlResult> {
  const result = await invoke<{
    job_id: number;
    enabled: boolean;
    state: string;
  }>("set_scheduled_job_enabled", { jobId, enabled });

  return {
    jobId: result.job_id,
    enabled: result.enabled,
    state: result.state,
  };
}

export async function deleteScheduledJob(
  jobId: number,
): Promise<ScheduledJobControlResult> {
  const result = await invoke<{
    job_id: number;
    enabled: boolean;
    state: string;
  }>("delete_scheduled_job", { jobId });

  return {
    jobId: result.job_id,
    enabled: result.enabled,
    state: result.state,
  };
}
