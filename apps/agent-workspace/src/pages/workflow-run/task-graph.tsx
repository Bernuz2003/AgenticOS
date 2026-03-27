import { Waypoints } from "lucide-react";

import type { OrchestrationStatus } from "../../lib/api";
import { WorkflowTaskList } from "./task-list";

interface WorkflowTaskGraphProps {
  detail: OrchestrationStatus;
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
}

export function WorkflowTaskGraph({
  detail,
  selectedTaskId,
  onSelectTask,
}: WorkflowTaskGraphProps) {
  return (
    <div className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="flex items-center gap-3">
        <div className="rounded-2xl bg-slate-100 p-3 text-slate-700">
          <Waypoints className="h-6 w-6" />
        </div>
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Execution Map
          </div>
          <h2 className="mt-1 text-xl font-bold text-slate-900">Task graph</h2>
        </div>
      </div>

      <div className="mt-5">
        <WorkflowTaskList
          tasks={detail.tasks}
          selectedTaskId={selectedTaskId}
          onSelectTask={onSelectTask}
        />
      </div>
    </div>
  );
}
