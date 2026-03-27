import type { OrchestrationStatus } from "../../lib/api";
import { WorkflowRunStatusBadge } from "../../components/workflows/run/status-badge";
import { formatReasonLabel } from "./format";

type WorkflowTask = OrchestrationStatus["tasks"][number];

interface WorkflowTaskListProps {
  tasks: WorkflowTask[];
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
}

export function WorkflowTaskList({
  tasks,
  selectedTaskId,
  onSelectTask,
}: WorkflowTaskListProps) {
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {tasks.map((task) => (
        <button
          key={task.task}
          type="button"
          onClick={() => onSelectTask(task.task)}
          className={`rounded-3xl border p-5 text-left shadow-sm transition ${
            selectedTaskId === task.task
              ? "border-indigo-200 bg-indigo-50/70"
              : "border-slate-200 bg-slate-50 hover:border-slate-300"
          }`}
        >
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-base font-semibold text-slate-900">{task.task}</div>
              <div className="mt-1 text-sm text-slate-500">
                {task.role ?? "Unassigned role"}
              </div>
            </div>
            <WorkflowRunStatusBadge status={task.status} />
          </div>

          <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              workload {task.workload ?? "default"}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              context {task.contextStrategy ?? "kernel_default"}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              deps {task.deps.length === 0 ? "root" : task.deps.join(", ")}
            </span>
          </div>

          {task.latestOutputPreview && (
            <div className="mt-4 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-xs leading-6 text-slate-600">
              {task.latestOutputPreview}
            </div>
          )}

          <div className="mt-4 flex flex-wrap gap-2 text-[11px] text-slate-500">
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              attempts {task.attempts.length}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
              artifacts {task.outputArtifacts.length}
            </span>
            {task.terminationReason && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                {formatReasonLabel(task.terminationReason)}
              </span>
            )}
            {task.currentAttempt !== null && (
              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                current {task.currentAttempt}
              </span>
            )}
          </div>
        </button>
      ))}
    </div>
  );
}
