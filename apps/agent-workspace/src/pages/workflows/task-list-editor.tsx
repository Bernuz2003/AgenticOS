import { ArrowRight, CalendarClock, Layers3, Waypoints } from "lucide-react";
import { Link } from "react-router-dom";

import type {
  DraftBackendClass,
  DraftContextStrategy,
  DraftTask,
  DraftWorkload,
  FailurePolicy,
  SchedulerDraft,
} from "../../lib/workflow-builder";
import {
  backendOptions,
  contextOptions,
  workloadOptions,
} from "../../lib/workflow-builder";
import {
  taskDisplayId,
  type WorkflowDraftValidation,
} from "../../lib/workflow-builder/graph";
import { formatSchedulerSummary } from "./scheduler-editor";

interface TaskListEditorProps {
  tasks: DraftTask[];
  rootTasksCount: number;
  failurePolicy: FailurePolicy;
  schedulerDraft: SchedulerDraft;
  validation: WorkflowDraftValidation;
  selectedTask: DraftTask | null;
  selectedTaskIndex: number | null;
  onUpdateTask: (index: number, patch: Partial<DraftTask>) => void;
  submittingMode: "launch" | "schedule" | null;
  onLaunchWorkflow: () => void;
  onScheduleWorkflow: () => void;
  onResetBuilder: () => void;
}

export function TaskListEditor({
  tasks,
  rootTasksCount,
  failurePolicy,
  schedulerDraft,
  validation,
  selectedTask,
  selectedTaskIndex,
  onUpdateTask,
  submittingMode,
  onLaunchWorkflow,
  onScheduleWorkflow,
  onResetBuilder,
}: TaskListEditorProps) {
  return (
    <aside className="space-y-6 xl:sticky xl:top-8 xl:h-fit">
      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="rounded-2xl bg-slate-100 p-3 text-slate-700">
            <Layers3 className="h-6 w-6" />
          </div>
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Draft Overview
            </div>
            <h2 className="mt-1 text-xl font-bold text-slate-900">
              {tasks.length} tasks ready
            </h2>
          </div>
        </div>

        <div className="mt-5 grid gap-3 sm:grid-cols-2">
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Failure Policy
            </div>
            <div className="mt-1 text-sm font-semibold text-slate-900">
              {failurePolicy}
            </div>
          </div>
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Root Tasks
            </div>
            <div className="mt-1 text-sm font-semibold text-slate-900">
              {rootTasksCount}
            </div>
          </div>
          <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 sm:col-span-2">
            <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
              Scheduler
            </div>
            <div className="mt-1 text-sm font-semibold text-slate-900">
              {formatSchedulerSummary(schedulerDraft)}
            </div>
          </div>
        </div>

        <div className="mt-5">
          <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
            Validation
          </div>
          {validation.errors.length === 0 && validation.warnings.length === 0 ? (
            <div className="mt-3 rounded-2xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700">
              Visual graph and workflow payload are aligned.
            </div>
          ) : (
            <div className="mt-3 space-y-3">
              {validation.errors.map((message) => (
                <div
                  key={`error:${message}`}
                  className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700"
                >
                  {message}
                </div>
              ))}
              {validation.warnings.map((message) => (
                <div
                  key={`warning:${message}`}
                  className="rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
                >
                  {message}
                </div>
              ))}
            </div>
          )}
        </div>
      </section>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Task Inspector
        </div>
        <h2 className="mt-2 text-xl font-bold text-slate-900">
          {selectedTask && selectedTaskIndex !== null
            ? taskDisplayId(selectedTask, selectedTaskIndex)
            : "No task selected"}
        </h2>

        {selectedTask && selectedTaskIndex !== null ? (
          <div className="mt-5 space-y-4">
            <div>
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Task ID
              </label>
              <input
                value={selectedTask.id}
                onChange={(event) =>
                  onUpdateTask(selectedTaskIndex, { id: event.target.value })
                }
                className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              />
            </div>

            <div>
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Role
              </label>
              <input
                value={selectedTask.role}
                onChange={(event) =>
                  onUpdateTask(selectedTaskIndex, { role: event.target.value })
                }
                placeholder="Analyst, Reviewer, Synthesizer"
                className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              />
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Workload
                </label>
                <select
                  value={selectedTask.workload}
                  onChange={(event) =>
                    onUpdateTask(selectedTaskIndex, {
                      workload: event.target.value as DraftWorkload,
                    })
                  }
                  className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                >
                  {workloadOptions.map((option) => (
                    <option key={option.value || "default"} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Runtime Target
                </label>
                <select
                  value={selectedTask.backendClass}
                  onChange={(event) =>
                    onUpdateTask(selectedTaskIndex, {
                      backendClass: event.target.value as DraftBackendClass,
                    })
                  }
                  className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                >
                  {backendOptions.map((option) => (
                    <option key={option.value || "auto"} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Context Strategy
                </label>
                <select
                  value={selectedTask.contextStrategy}
                  onChange={(event) =>
                    onUpdateTask(selectedTaskIndex, {
                      contextStrategy: event.target.value as DraftContextStrategy,
                    })
                  }
                  className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                >
                  {contextOptions.map((option) => (
                    <option key={option.value || "kernel_default"} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div>
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Dependencies
              </label>
              <input
                value={selectedTask.depsText}
                onChange={(event) =>
                  onUpdateTask(selectedTaskIndex, { depsText: event.target.value })
                }
                placeholder="Comma-separated task ids"
                className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              />
            </div>

            <div>
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Prompt
              </label>
              <textarea
                value={selectedTask.prompt}
                onChange={(event) =>
                  onUpdateTask(selectedTaskIndex, { prompt: event.target.value })
                }
                className="min-h-[180px] w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm leading-relaxed text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              />
            </div>
          </div>
        ) : (
          <div className="mt-4 rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-sm text-slate-500">
            Select a node from the visual builder to edit its details.
          </div>
        )}
      </section>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Actions
        </div>
        <h2 className="mt-2 text-lg font-bold text-slate-900">
          Launch or schedule this graph
        </h2>

        <div className="mt-5 flex flex-col gap-3">
          <button
            type="button"
            onClick={onLaunchWorkflow}
            disabled={submittingMode !== null || validation.errors.length > 0}
            className="inline-flex items-center justify-center gap-2 rounded-xl bg-slate-900 px-5 py-3 text-sm font-semibold text-white hover:bg-slate-800 disabled:opacity-40"
          >
            <Waypoints className="h-4 w-4" />
            {submittingMode === "launch" ? "Launching..." : "Launch workflow"}
          </button>
          <button
            type="button"
            onClick={onScheduleWorkflow}
            disabled={submittingMode !== null || validation.errors.length > 0}
            className="inline-flex items-center justify-center gap-2 rounded-xl border border-indigo-200 bg-indigo-50 px-5 py-3 text-sm font-semibold text-indigo-700 hover:bg-indigo-100 disabled:opacity-40"
          >
            <CalendarClock className="h-4 w-4" />
            {submittingMode === "schedule" ? "Scheduling..." : "Schedule job"}
          </button>
          <button
            type="button"
            onClick={onResetBuilder}
            disabled={submittingMode !== null}
            className="rounded-xl border border-slate-200 bg-white px-5 py-3 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-40"
          >
            Reset draft
          </button>
        </div>

        {validation.errors.length > 0 && (
          <div className="mt-4 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900">
            Resolve validation errors before launch or scheduling.
          </div>
        )}
      </section>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-center gap-3">
          <div className="rounded-2xl bg-amber-50 p-3 text-amber-700">
            <CalendarClock className="h-6 w-6" />
          </div>
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Runtime separation
            </div>
            <h2 className="mt-1 text-lg font-bold text-slate-900">
              Monitoring lives in Jobs
            </h2>
          </div>
        </div>
        <p className="mt-4 text-sm leading-6 text-slate-600">
          Workflows is now the design surface. Live orchestrations, scheduled jobs
          and destructive controls stay in the dedicated runtime view.
        </p>
        <Link
          to="/jobs"
          className="mt-5 inline-flex items-center gap-2 rounded-xl bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 ring-1 ring-slate-200 hover:bg-slate-50"
        >
          Open Jobs
          <ArrowRight className="h-4 w-4" />
        </Link>
      </section>
    </aside>
  );
}
