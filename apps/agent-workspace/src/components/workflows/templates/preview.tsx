import { ArrowRight, CalendarClock, Sparkles } from "lucide-react";

import type { WorkflowTemplateDefinition } from "../../../lib/workflow-templates";

interface WorkflowTemplatePreviewProps {
  template: WorkflowTemplateDefinition | null;
  onApply: (templateId: string) => void;
}

export function WorkflowTemplatePreview({
  template,
  onApply,
}: WorkflowTemplatePreviewProps) {
  return (
    <aside className="h-fit rounded-3xl border border-slate-200 bg-white p-6 shadow-sm xl:sticky xl:top-8">
      {template ? (
        <>
          <div className="flex items-center gap-3">
            <div className="rounded-2xl bg-indigo-50 p-3 text-indigo-600">
              <Sparkles className="h-6 w-6" />
            </div>
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Template Preview
              </div>
              <h2 className="mt-1 text-xl font-bold text-slate-900">{template.name}</h2>
            </div>
          </div>

          <p className="mt-4 text-sm leading-6 text-slate-600">{template.description}</p>

          <div className="mt-5 rounded-2xl border border-slate-200 bg-slate-50 p-4">
            <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
              Execution shape
            </div>
            <div className="mt-3 space-y-3">
              {template.tasks.map((task, index) => (
                <div
                  key={task.id}
                  className="rounded-2xl border border-slate-200 bg-white px-4 py-3"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">{task.id}</div>
                      <div className="text-xs text-slate-500">{task.role}</div>
                    </div>
                    <div className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                      {index + 1}
                    </div>
                  </div>
                  <div className="mt-2 text-xs leading-5 text-slate-600">{task.prompt}</div>
                  <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                      workload {task.workload ?? "default"}
                    </span>
                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                      deps {(task.deps ?? []).length === 0 ? "root" : task.deps?.join(", ")}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>

          {template.schedulerPreset && (
            <div className="mt-5 rounded-2xl border border-amber-200 bg-amber-50 p-4">
              <div className="flex items-center gap-2 text-amber-700">
                <CalendarClock className="h-4 w-4" />
                <span className="text-xs font-bold uppercase tracking-[0.18em]">
                  Scheduler preset included
                </span>
              </div>
              <div className="mt-2 text-sm text-amber-950">
                This template ships with a ready-to-edit scheduler configuration.
              </div>
            </div>
          )}

          <button
            type="button"
            onClick={() => onApply(template.id)}
            className="mt-6 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-slate-900 px-4 py-3 text-sm font-semibold text-white hover:bg-slate-800"
          >
            Use this template
            <ArrowRight className="h-4 w-4" />
          </button>
        </>
      ) : (
        <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
          No template matches the current filters.
        </div>
      )}
    </aside>
  );
}
