import { Plus } from "lucide-react";

import type { DraftTask } from "../../../lib/workflow-builder";
import { taskDisplayId } from "../../../lib/workflow-builder/graph";

interface WorkflowCanvasInspectorProps {
  tasks: DraftTask[];
  linkSourceIndex: number | null;
  onAddTask: () => void;
}

export function WorkflowCanvasInspector({
  tasks,
  linkSourceIndex,
  onAddTask,
}: WorkflowCanvasInspectorProps) {
  return (
    <div className="mb-4 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
      <div>
        <div className="text-xs font-bold uppercase tracking-[0.18em] text-slate-400">
          Visual DAG Builder
        </div>
        <h3 className="mt-1 text-lg font-bold text-slate-900">
          Drag nodes to arrange the graph. Drag a link handle to create dependencies.
        </h3>
      </div>
      <div className="flex flex-wrap gap-3">
        {linkSourceIndex !== null && tasks[linkSourceIndex] && (
          <div className="rounded-xl border border-indigo-200 bg-indigo-50 px-3 py-2 text-xs font-semibold text-indigo-700">
            Linking from {taskDisplayId(tasks[linkSourceIndex], linkSourceIndex)}
          </div>
        )}
        <button
          type="button"
          onClick={onAddTask}
          className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
        >
          <Plus className="h-4 w-4" />
          Add task
        </button>
      </div>
    </div>
  );
}
