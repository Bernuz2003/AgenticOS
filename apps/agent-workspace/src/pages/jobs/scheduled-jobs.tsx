import { PlayCircle, ShieldOff, Trash2, Waypoints } from "lucide-react";
import { Link } from "react-router-dom";

import type { ScheduledJob } from "../../lib/api";
import { formatElapsed, formatTimestamp, statusTone } from "./controls";

interface ScheduledJobsProps {
  scheduledJobs: ScheduledJob[];
  busyKey: string | null;
  onToggleJob: (jobId: number, enabled: boolean) => void;
  onDeleteJob: (jobId: number) => void;
}

export function ScheduledJobs({
  scheduledJobs,
  busyKey,
  onToggleJob,
  onDeleteJob,
}: ScheduledJobsProps) {
  return (
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
                    onClick={() => onToggleJob(job.jobId, !job.enabled)}
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
                    onClick={() => onDeleteJob(job.jobId)}
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
  );
}
