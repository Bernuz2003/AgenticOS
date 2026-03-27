import type { Dispatch, SetStateAction } from "react";

import { WorkflowCanvasBuilder } from "../../components/workflows/canvas-builder";
import type {
  DraftTask,
  FailurePolicy,
  SchedulerDraft,
} from "../../lib/workflow-builder";
import { SchedulerEditor } from "./scheduler-editor";

interface BuilderPaneProps {
  failurePolicy: FailurePolicy;
  onFailurePolicyChange: (value: FailurePolicy) => void;
  draftSourceTemplateName: string | null;
  tasks: DraftTask[];
  selectedTaskIndex: number | null;
  onSelectTask: Dispatch<SetStateAction<number | null>>;
  onTasksChange: Dispatch<SetStateAction<DraftTask[]>>;
  onAddTask: () => void;
  onRemoveTask: (index: number) => void;
  schedulerDraft: SchedulerDraft;
  setSchedulerDraft: Dispatch<SetStateAction<SchedulerDraft>>;
  submitError: string | null;
}

export function BuilderPane({
  failurePolicy,
  onFailurePolicyChange,
  draftSourceTemplateName,
  tasks,
  selectedTaskIndex,
  onSelectTask,
  onTasksChange,
  onAddTask,
  onRemoveTask,
  schedulerDraft,
  setSchedulerDraft,
  submitError,
}: BuilderPaneProps) {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Workflow Builder
          </div>
          <h2 className="mt-2 text-2xl font-bold text-slate-900">
            Compose the task graph
          </h2>
          <p className="mt-2 max-w-2xl text-sm text-slate-500">
            Keep the current form-based workflow creation flow, then launch it
            immediately or persist it as a scheduled job.
          </p>
        </div>
        <div className="w-full max-w-xs">
          <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
            Failure Policy
          </label>
          <select
            value={failurePolicy}
            onChange={(event) => onFailurePolicyChange(event.target.value as FailurePolicy)}
            className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
          >
            <option value="fail_fast">Fail fast</option>
            <option value="best_effort">Best effort</option>
          </select>
        </div>
      </div>

      {draftSourceTemplateName && (
        <div className="mt-5 rounded-2xl border border-indigo-200 bg-indigo-50 px-4 py-3 text-sm text-indigo-900">
          Builder initialized from template <strong>{draftSourceTemplateName}</strong>. You
          can now customize tasks and scheduler before launch.
        </div>
      )}

      <div className="mt-6">
        <WorkflowCanvasBuilder
          tasks={tasks}
          selectedTaskIndex={selectedTaskIndex}
          onSelectTask={onSelectTask}
          onTasksChange={onTasksChange}
          onAddTask={onAddTask}
          onRemoveTask={onRemoveTask}
        />
      </div>

      <SchedulerEditor
        schedulerDraft={schedulerDraft}
        setSchedulerDraft={setSchedulerDraft}
      />

      {submitError && (
        <div className="mt-5 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {submitError}
        </div>
      )}
    </section>
  );
}
