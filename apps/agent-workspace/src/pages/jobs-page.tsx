import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  ArrowRight,
  Clock3,
  PauseCircle,
  PlayCircle,
  RefreshCw,
  ShieldOff,
  Trash2,
  Waypoints,
} from "lucide-react";
import {
  deleteScheduledJob,
  deleteWorkflowRun,
  fetchOrchestrationStatus,
  setScheduledJobEnabled,
  stopWorkflowRun,
  type OrchestrationStatus,
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";

function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

function formatElapsed(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 1) {
    return "<1s";
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function progressPercent(total: number, completed: number, failed: number, skipped: number): number {
  if (total <= 0) {
    return 0;
  }
  return Math.round(((completed + failed + skipped) / total) * 100);
}

function statusTone(status: string): string {
  switch (status) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "failed":
    case "cancelled":
      return "border-rose-200 bg-rose-50 text-rose-700";
    case "retry_wait":
    case "skipped":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "disabled":
      return "border-slate-200 bg-slate-100 text-slate-600";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
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
      <header className="rounded-[32px] border border-slate-200 bg-white px-8 py-8 shadow-sm">
        <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-3xl">
            <div className="text-xs font-bold uppercase tracking-[0.28em] text-slate-400">
              Jobs Runtime
            </div>
            <h1 className="mt-3 text-3xl font-bold tracking-tight text-slate-900">
              Live runs, run history and scheduled jobs
            </h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              This is the operational surface for workflow execution. Stop and delete
              runs here, inspect scheduled jobs here, and keep `Workflows` focused on
              design instead of monitoring.
            </p>
          </div>
          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              onClick={() => void handleRefresh()}
              className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
            <Link
              to="/workflows"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Open Workflows
            </Link>
            <Link
              to="/control-center"
              className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
            >
              Control Center
              <ArrowRight className="h-4 w-4" />
            </Link>
          </div>
        </div>

        <div className="mt-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-5 py-4">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Live Runs
            </div>
            <div className="mt-1 text-2xl font-bold text-slate-900">{liveRuns.length}</div>
          </div>
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-5 py-4">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Run History
            </div>
            <div className="mt-1 text-2xl font-bold text-slate-900">{finishedRuns.length}</div>
          </div>
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-5 py-4">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Active Jobs
            </div>
            <div className="mt-1 text-2xl font-bold text-slate-900">{activeJobs.length}</div>
          </div>
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-5 py-4">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Disabled Jobs
            </div>
            <div className="mt-1 text-2xl font-bold text-slate-900">{disabledJobs.length}</div>
          </div>
        </div>
      </header>

      {actionError && (
        <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {actionError}
        </div>
      )}

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Live Orchestrations
            </div>
            <h2 className="mt-2 text-2xl font-bold text-slate-900">
              Running workflow runs
            </h2>
          </div>
          <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
            {liveRuns.length} active
          </div>
        </div>

        {liveRuns.length === 0 ? (
          <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
            No live workflow runs. Launch one from the Workflows page.
          </div>
        ) : (
          <div className="mt-6 grid gap-4 xl:grid-cols-2">
            {liveRuns.map((workflow) => {
              const detail = workflowDetails[workflow.orchestrationId];
              const progress = progressPercent(
                workflow.total,
                workflow.completed,
                workflow.failed,
                workflow.skipped,
              );
              const runningTasks =
                detail?.tasks.filter((task) => task.status === "running").map((task) => task.task) ??
                [];
              return (
                <article
                  key={workflow.orchestrationId}
                  className="rounded-3xl border border-slate-200 bg-slate-50 p-5"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                        Workflow Run
                      </div>
                      <h3 className="mt-2 text-xl font-bold text-slate-900">
                        Run #{workflow.orchestrationId}
                      </h3>
                      <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                        <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 font-semibold text-indigo-700">
                          {workflow.policy}
                        </span>
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                          elapsed {workflow.elapsedLabel}
                        </span>
                      </div>
                    </div>
                    <div className="rounded-2xl border border-slate-200 bg-white px-4 py-3 text-right">
                      <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                        Progress
                      </div>
                      <div className="mt-1 text-xl font-bold text-slate-900">{progress}%</div>
                    </div>
                  </div>

                  <div className="mt-4 h-2 overflow-hidden rounded-full bg-slate-200">
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-indigo-500 to-sky-400"
                      style={{ width: `${progress}%` }}
                    />
                  </div>

                  <div className="mt-5 grid gap-3 sm:grid-cols-5">
                    {[
                      ["Total", workflow.total, "text-slate-900"],
                      ["Running", workflow.running, "text-emerald-700"],
                      ["Done", workflow.completed, "text-sky-700"],
                      ["Pending", workflow.pending, "text-amber-700"],
                      ["Failed", workflow.failed, "text-rose-700"],
                    ].map(([label, value, tone]) => (
                      <div
                        key={label}
                        className="rounded-2xl border border-slate-200 bg-white px-3 py-3 text-center"
                      >
                        <div className="text-[10px] font-bold uppercase tracking-wider text-slate-400">
                          {label}
                        </div>
                        <div className={`mt-1 text-lg font-bold ${tone}`}>{value}</div>
                      </div>
                    ))}
                  </div>

                  <div className="mt-5 rounded-2xl border border-slate-200 bg-white px-4 py-4">
                    <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                      <Clock3 className="h-4 w-4 text-slate-400" />
                      Currently active tasks
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2">
                      {runningTasks.length === 0 ? (
                        <span className="text-sm text-slate-500">
                          Detail still syncing or no active task reported.
                        </span>
                      ) : (
                        runningTasks.map((task) => (
                          <span
                            key={task}
                            className="rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1 text-[11px] font-semibold text-emerald-700"
                          >
                            {task}
                          </span>
                        ))
                      )}
                    </div>
                  </div>

                  <div className="mt-5 flex flex-wrap gap-3">
                    <Link
                      to={`/workflow-runs/${workflow.orchestrationId}`}
                      className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
                    >
                      Open run
                      <ArrowRight className="h-4 w-4" />
                    </Link>
                    <button
                      type="button"
                      onClick={() => void handleStopRun(workflow.orchestrationId)}
                      disabled={busyKey === `run:stop:${workflow.orchestrationId}`}
                      className="inline-flex items-center gap-2 rounded-xl border border-amber-200 bg-amber-50 px-4 py-2.5 text-sm font-semibold text-amber-700 hover:bg-amber-100 disabled:opacity-40"
                    >
                      <PauseCircle className="h-4 w-4" />
                      {busyKey === `run:stop:${workflow.orchestrationId}` ? "Stopping..." : "Stop"}
                    </button>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>

      <section className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
        <div className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Run History
              </div>
              <h2 className="mt-2 text-2xl font-bold text-slate-900">
                Finished workflow runs
              </h2>
            </div>
            <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
              {finishedRuns.length} retained
            </div>
          </div>

          {finishedRuns.length === 0 ? (
            <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
              No finished workflow runs retained yet.
            </div>
          ) : (
            <div className="mt-6 space-y-3">
              {finishedRuns.map((workflow) => {
                const detail = workflowDetails[workflow.orchestrationId];
                return (
                  <article
                    key={workflow.orchestrationId}
                    className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
                  >
                    <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
                      <div>
                        <div className="flex flex-wrap items-center gap-2">
                          <div className="text-base font-semibold text-slate-900">
                            Run #{workflow.orchestrationId}
                          </div>
                          <span
                            className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${statusTone(
                              workflow.failed > 0 ? "failed" : "completed",
                            )}`}
                          >
                            {workflow.failed > 0 ? "failed" : "completed"}
                          </span>
                        </div>
                        <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                            elapsed {workflow.elapsedLabel}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                            tasks {workflow.completed + workflow.failed + workflow.skipped}/{workflow.total}
                          </span>
                          {detail && (
                            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                              stored chars {detail.outputCharsStored.toLocaleString()}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-3">
                        <Link
                          to={`/workflow-runs/${workflow.orchestrationId}`}
                          className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
                        >
                          Open
                          <ArrowRight className="h-4 w-4" />
                        </Link>
                        <button
                          type="button"
                          onClick={() => void handleDeleteRun(workflow.orchestrationId)}
                          disabled={busyKey === `run:delete:${workflow.orchestrationId}`}
                          className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-4 py-2.5 text-sm font-semibold text-rose-700 hover:bg-rose-100 disabled:opacity-40"
                        >
                          <Trash2 className="h-4 w-4" />
                          {busyKey === `run:delete:${workflow.orchestrationId}` ? "Deleting..." : "Delete"}
                        </button>
                      </div>
                    </div>
                  </article>
                );
              })}
            </div>
          )}
        </div>

        <div className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Scheduled Jobs
              </div>
              <h2 className="mt-2 text-2xl font-bold text-slate-900">
                Durable job definitions
              </h2>
            </div>
            <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
              {scheduledJobs.length} jobs
            </div>
          </div>

          {scheduledJobs.length === 0 ? (
            <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
              No scheduled jobs stored yet.
            </div>
          ) : (
            <div className="mt-6 space-y-3">
              {scheduledJobs.map((job) => (
                <article
                  key={job.jobId}
                  className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
                >
                  <div className="flex flex-col gap-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <div className="flex flex-wrap items-center gap-2">
                          <div className="text-base font-semibold text-slate-900">{job.name}</div>
                          <span
                            className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${statusTone(
                              job.state,
                            )}`}
                          >
                            {job.state}
                          </span>
                          {!job.enabled && (
                            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                              disabled
                            </span>
                          )}
                        </div>
                        <div className="mt-2 text-sm text-slate-500">{job.triggerLabel}</div>
                      </div>

                      <div className="rounded-2xl border border-slate-200 bg-white px-4 py-3 text-right">
                        <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                          Next Run
                        </div>
                        <div className="mt-1 text-sm font-semibold text-slate-900">
                          {formatTimestamp(job.nextRunAtMs)}
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-wrap gap-2 text-[11px] text-slate-500">
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        timeout {formatElapsed(job.timeoutMs / 1000)}
                      </span>
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        retries {job.maxRetries}
                      </span>
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        backoff {formatElapsed(job.backoffMs / 1000)}
                      </span>
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        last {job.lastRunStatus ?? "never"}
                      </span>
                    </div>

                    {job.lastError && (
                      <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
                        {job.lastError}
                      </div>
                    )}

                    {job.activeOrchestrationId !== null && (
                      <Link
                        to={`/workflow-runs/${job.activeOrchestrationId}`}
                        className="inline-flex items-center gap-2 text-sm font-semibold text-indigo-700 hover:text-indigo-900"
                      >
                        <Waypoints className="h-4 w-4" />
                        Open active run #{job.activeOrchestrationId}
                      </Link>
                    )}

                    <div className="flex flex-wrap gap-3">
                      <button
                        type="button"
                        onClick={() => void handleToggleJob(job.jobId, !job.enabled)}
                        disabled={busyKey === `job:toggle:${job.jobId}`}
                        className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-40"
                      >
                        {job.enabled ? (
                          <>
                            <ShieldOff className="h-4 w-4" />
                            {busyKey === `job:toggle:${job.jobId}` ? "Disabling..." : "Disable"}
                          </>
                        ) : (
                          <>
                            <PlayCircle className="h-4 w-4" />
                            {busyKey === `job:toggle:${job.jobId}` ? "Enabling..." : "Enable"}
                          </>
                        )}
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleDeleteJob(job.jobId)}
                        disabled={busyKey === `job:delete:${job.jobId}`}
                        className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-4 py-2.5 text-sm font-semibold text-rose-700 hover:bg-rose-100 disabled:opacity-40"
                      >
                        <Trash2 className="h-4 w-4" />
                        {busyKey === `job:delete:${job.jobId}` ? "Deleting..." : "Delete"}
                      </button>
                    </div>
                  </div>
                </article>
              ))}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
