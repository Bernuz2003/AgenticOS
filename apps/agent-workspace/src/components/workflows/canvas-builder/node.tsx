import { Link2, Trash2 } from "lucide-react";

import type { DraftTask } from "../../../lib/workflow-builder";
import { removeDependency, taskDisplayId } from "../../../lib/workflow-builder/graph";

const NODE_WIDTH = 248;
const NODE_HEIGHT = 138;

function workloadTone(workload: DraftTask["workload"]): string {
  switch (workload) {
    case "reasoning":
      return "border-indigo-200 bg-indigo-50 text-indigo-700";
    case "code":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "fast":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-600";
  }
}

interface WorkflowCanvasNodeProps {
  task: DraftTask;
  index: number;
  position: { x: number; y: number };
  tasks: DraftTask[];
  selected: boolean;
  linkSource: boolean;
  onSelect: (index: number) => void;
  onStartDrag: (index: number, clientX: number, clientY: number, rect: DOMRect) => void;
  onStartLink: (index: number, clientX: number, clientY: number) => void;
  onRemoveTask: (index: number) => void;
  onTasksChange: (tasks: DraftTask[]) => void;
}

export function WorkflowCanvasNode({
  task,
  index,
  position,
  tasks,
  selected,
  linkSource,
  onSelect,
  onStartDrag,
  onStartLink,
  onRemoveTask,
  onTasksChange,
}: WorkflowCanvasNodeProps) {
  const deps = task.depsText
    .split(/[,\n]/)
    .map((value) => value.trim())
    .filter(Boolean);

  return (
    <article
      data-workflow-node-index={index}
      className={`absolute rounded-[28px] border p-4 shadow-sm transition ${
        selected
          ? "border-indigo-200 bg-indigo-50/90 shadow-indigo-100"
          : linkSource
            ? "border-indigo-200 bg-white shadow-indigo-100"
            : "border-slate-200 bg-white hover:border-slate-300"
      }`}
      style={{
        width: NODE_WIDTH,
        minHeight: NODE_HEIGHT,
        transform: `translate(${position.x}px, ${position.y}px)`,
      }}
      onClick={() => onSelect(index)}
      onPointerDown={(event) => {
        const target = event.target as HTMLElement;
        if (target.closest("button")) {
          return;
        }
        const rect = target.closest("article")?.getBoundingClientRect();
        if (!rect) {
          return;
        }
        onSelect(index);
        onStartDrag(index, event.clientX, event.clientY, rect);
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-base font-semibold text-slate-900">
            {taskDisplayId(task, index)}
          </div>
          <div className="mt-1 text-sm text-slate-500">
            {task.role.trim() || "Unassigned role"}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            title="Start linking from this task"
            onPointerDown={(event) => {
              event.preventDefault();
              event.stopPropagation();
              onSelect(index);
              onStartLink(index, event.clientX, event.clientY);
            }}
            className="rounded-xl border border-indigo-200 bg-indigo-50 p-2 text-indigo-700 hover:bg-indigo-100"
          >
            <Link2 className="h-4 w-4" />
          </button>
          <button
            type="button"
            title="Remove task"
            disabled={tasks.length === 1}
            onClick={(event) => {
              event.stopPropagation();
              onRemoveTask(index);
            }}
            className="rounded-xl border border-slate-200 bg-slate-50 p-2 text-slate-500 hover:text-rose-600 disabled:opacity-40"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        <span
          className={`rounded-full border px-2.5 py-1 text-[11px] font-semibold ${workloadTone(
            task.workload,
          )}`}
        >
          {task.workload || "general"}
        </span>
        <span className="rounded-full border border-slate-200 bg-slate-100 px-2.5 py-1 text-[11px] font-semibold text-slate-600">
          {deps.length === 0 ? "root" : `${deps.length} deps`}
        </span>
      </div>

      <div className="mt-4 line-clamp-3 text-sm leading-6 text-slate-600">
        {task.prompt.trim() || "No prompt yet."}
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        {deps.length === 0 ? (
          <span className="rounded-full border border-dashed border-slate-200 px-2.5 py-1 text-[11px] text-slate-400">
            No dependencies
          </span>
        ) : (
          deps.map((depId) => (
            <button
              key={`${taskDisplayId(task, index)}:${depId}`}
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                onTasksChange(removeDependency(tasks, index, depId));
              }}
              className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[11px] font-semibold text-slate-600 hover:border-rose-200 hover:text-rose-700"
            >
              {depId} ×
            </button>
          ))
        )}
      </div>
    </article>
  );
}

export { NODE_HEIGHT, NODE_WIDTH };
