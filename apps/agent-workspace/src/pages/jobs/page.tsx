import { useEffect, useMemo, useState } from "react";

import {
  deleteScheduledJob,
  deleteWorkflowRun,
  fetchOrchestrationStatus,
  setScheduledJobEnabled,
  stopWorkflowRun,
  type OrchestrationStatus,
} from "../../lib/api";
import { useSessionsStore } from "../../store/sessions-store";
import { JobsControls } from "./controls";
import { LiveOrchestrations } from "./live-orchestrations";
import { ScheduledJobs } from "./scheduled-jobs";

export function progressPercent(
  total: number,
  completed: number,
  failed: number,
  skipped: number,
): number {
  if (total <= 0) {
    return 0;
  }
  return Math.round(((completed + failed + skipped) / total) * 100);
}

export function primaryTerminationReason(
  detail: OrchestrationStatus | undefined,
): string | null {
  if (!detail) {
    return null;
  }
  const reasons = detail.tasks
    .flatMap((task) => task.attempts.map((attempt) => attempt.terminationReason))
    .filter((reason): reason is string => Boolean(reason));
  return reasons[0] ?? null;
}

export function formatReasonLabel(reason: string | null): string {
  return reason ? reason.split("_").join(" ") : "n/a";
}

export function JobsPage() {
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const scheduledJobs = useSessionsStore((state) => state.scheduledJobs);
  const refreshLobby = useSessionsStore((state) => state.refresh);
  const [workflowDetails, setWorkflowDetails] = useState<Record<number, OrchestrationStatus>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);

  const orchestrationSignature = useMemo(
    () =>
      orchestrations
        .map(
          (workflow) =>
            `${workflow.orchestrationId}:${workflow.running}:${workflow.pending}:${workflow.completed}:${workflow.failed}:${workflow.skipped}:${workflow.finished}`,
        )
        .join("|"),
    [orchestrations],
  );

  useEffect(() => {
    if (orchestrations.length === 0) {
      setWorkflowDetails({});
      return;
    }

    let cancelled = false;
    const load = async () => {
      const results = await Promise.allSettled(
        orchestrations.map(async (workflow) => [
          workflow.orchestrationId,
          await fetchOrchestrationStatus(workflow.orchestrationId),
        ] as const),
      );
      if (cancelled) {
        return;
      }

      const nextDetails: Record<number, OrchestrationStatus> = {};
      for (const result of results) {
        if (result.status === "fulfilled") {
          const [orchestrationId, detail] = result.value;
          nextDetails[orchestrationId] = detail;
        }
      }
      setWorkflowDetails(nextDetails);
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [orchestrationSignature, orchestrations]);

  const liveRuns = orchestrations.filter((workflow) => !workflow.finished);
  const finishedRuns = orchestrations.filter((workflow) => workflow.finished);
  const activeJobs = scheduledJobs.filter(
    (job) => job.enabled || job.activeOrchestrationId !== null,
  );
  const disabledJobs = scheduledJobs.filter(
    (job) => !job.enabled && job.activeOrchestrationId === null,
  );

  async function handleRefresh() {
    setActionError(null);
    await refreshLobby();
  }

  async function handleStopRun(orchestrationId: number) {
    const key = `run:stop:${orchestrationId}`;
    setBusyKey(key);
    setActionError(null);
    try {
      await stopWorkflowRun(orchestrationId);
      await refreshLobby();
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "Failed to stop workflow run",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function handleDeleteRun(orchestrationId: number) {
    const key = `run:delete:${orchestrationId}`;
    setBusyKey(key);
    setActionError(null);
    try {
      await deleteWorkflowRun(orchestrationId);
      await refreshLobby();
      setWorkflowDetails((current) => {
        const next = { ...current };
        delete next[orchestrationId];
        return next;
      });
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "Failed to delete workflow run",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function handleToggleJob(jobId: number, enabled: boolean) {
    const key = `job:toggle:${jobId}`;
    setBusyKey(key);
    setActionError(null);
    try {
      await setScheduledJobEnabled(jobId, enabled);
      await refreshLobby();
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "Failed to update scheduled job",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function handleDeleteJob(jobId: number) {
    const key = `job:delete:${jobId}`;
    setBusyKey(key);
    setActionError(null);
    try {
      await deleteScheduledJob(jobId);
      await refreshLobby();
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "Failed to delete scheduled job",
      );
    } finally {
      setBusyKey(null);
    }
  }

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <JobsControls
        liveRunsCount={liveRuns.length}
        finishedRunsCount={finishedRuns.length}
        activeJobsCount={activeJobs.length}
        disabledJobsCount={disabledJobs.length}
        actionError={actionError}
        onRefresh={() => void handleRefresh()}
      />

      <section className="grid gap-6">
        <LiveOrchestrations
          liveRuns={liveRuns}
          finishedRuns={finishedRuns}
          workflowDetails={workflowDetails}
          busyKey={busyKey}
          onStopRun={(orchestrationId) => void handleStopRun(orchestrationId)}
          onDeleteRun={(orchestrationId) => void handleDeleteRun(orchestrationId)}
        />

        <ScheduledJobs
          scheduledJobs={scheduledJobs}
          busyKey={busyKey}
          onToggleJob={(jobId, enabled) => void handleToggleJob(jobId, enabled)}
          onDeleteJob={(jobId) => void handleDeleteJob(jobId)}
        />
      </section>
    </div>
  );
}
