import { ArrowRight, Clock3, PauseCircle, Trash2 } from "lucide-react";
import { Link } from "react-router-dom";

import type { OrchestrationStatus } from "../../lib/api";
import { formatReasonLabel, primaryTerminationReason, progressPercent } from "./page";
import { statusTone } from "./controls";

interface WorkflowRunSummary {
  orchestrationId: number;
  policy: string;
  elapsedLabel: string;
  total: number;
  running: number;
  completed: number;
  pending: number;
  failed: number;
  skipped: number;
  finished: boolean;
}

interface LiveOrchestrationsProps {
  liveRuns: WorkflowRunSummary[];
  finishedRuns: WorkflowRunSummary[];
  workflowDetails: Record<number, OrchestrationStatus>;
  busyKey: string | null;
  onStopRun: (orchestrationId: number) => void;
  onDeleteRun: (orchestrationId: number) => void;
}

export function LiveOrchestrations({
  liveRuns,
  finishedRuns,
  workflowDetails,
  busyKey,
  onStopRun,
  onDeleteRun,
}: LiveOrchestrationsProps) {
  return (
    <>
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
              const terminationReason = primaryTerminationReason(detail);
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
                        {terminationReason && (
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                            {formatReasonLabel(terminationReason)}
                          </span>
                        )}
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
                      onClick={() => onStopRun(workflow.orchestrationId)}
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
              const terminationReason = primaryTerminationReason(detail);
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
                        {terminationReason && (
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                            {formatReasonLabel(terminationReason)}
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
                        onClick={() => onDeleteRun(workflow.orchestrationId)}
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
    </>
  );
}
