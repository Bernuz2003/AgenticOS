import { ArrowRight } from "lucide-react";

import type { WorkflowTemplateDefinition } from "../../../lib/workflow-templates";

function templateCategoryTone(category: string): string {
  switch (category) {
    case "Coding":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "Automation":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "Research":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "Compare":
      return "border-violet-200 bg-violet-50 text-violet-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

interface WorkflowTemplateCardProps {
  template: WorkflowTemplateDefinition;
  selected: boolean;
  onPreview: (templateId: string) => void;
  onApply: (templateId: string) => void;
}

export function WorkflowTemplateCard({
  template,
  selected,
  onPreview,
  onApply,
}: WorkflowTemplateCardProps) {
  return (
    <article
      className={`rounded-3xl border p-5 shadow-sm transition ${
        selected
          ? "border-indigo-200 bg-indigo-50/60"
          : "border-slate-200 bg-white hover:-translate-y-0.5 hover:border-slate-300"
      }`}
    >
      <div className="flex items-start justify-between gap-4">
        <div>
          <div
            className={`inline-flex rounded-full border px-3 py-1 text-[11px] font-bold uppercase tracking-wider ${templateCategoryTone(
              template.category,
            )}`}
          >
            {template.category}
          </div>
          <h3 className="mt-3 text-lg font-bold text-slate-900">{template.name}</h3>
          <p className="mt-2 text-sm leading-6 text-slate-600">{template.summary}</p>
        </div>
        <div className="rounded-2xl bg-white px-3 py-2 text-right shadow-sm">
          <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
            Tasks
          </div>
          <div className="mt-1 text-lg font-bold text-slate-900">{template.tasks.length}</div>
        </div>
      </div>

      <div className="mt-4 flex flex-wrap gap-2">
        {template.tags.map((tag) => (
          <span
            key={tag}
            className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[11px] font-semibold text-slate-600"
          >
            {tag}
          </span>
        ))}
      </div>

      <div className="mt-4 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-600">
        <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
          Recommended runtime
        </div>
        <div className="mt-1 font-semibold text-slate-800">{template.recommendedRuntime}</div>
      </div>

      <div className="mt-5 flex flex-wrap gap-3">
        <button
          type="button"
          onClick={() => onPreview(template.id)}
          className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50"
        >
          Preview
        </button>
        <button
          type="button"
          onClick={() => onApply(template.id)}
          className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white hover:bg-slate-800"
        >
          Use template
          <ArrowRight className="h-4 w-4" />
        </button>
      </div>
    </article>
  );
}
