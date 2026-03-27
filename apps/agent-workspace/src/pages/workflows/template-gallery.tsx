import { Search } from "lucide-react";

import { WorkflowTemplateGrid } from "../../components/workflows/templates/grid";
import { WorkflowTemplatePreview } from "../../components/workflows/templates/preview";
import type { WorkflowTemplateDefinition } from "../../lib/workflow-templates";

interface TemplateGalleryProps {
  categories: string[];
  filteredTemplates: WorkflowTemplateDefinition[];
  selectedTemplateId: string;
  selectedTemplate: WorkflowTemplateDefinition | null;
  templateQuery: string;
  templateCategory: string;
  onTemplateQueryChange: (value: string) => void;
  onTemplateCategoryChange: (value: string) => void;
  onSelectTemplate: (templateId: string) => void;
  onApplyTemplate: (templateId: string) => void;
}

export function TemplateGallery({
  categories,
  filteredTemplates,
  selectedTemplateId,
  selectedTemplate,
  templateQuery,
  templateCategory,
  onTemplateQueryChange,
  onTemplateCategoryChange,
  onSelectTemplate,
  onApplyTemplate,
}: TemplateGalleryProps) {
  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_380px]">
      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Template Library
            </div>
            <h2 className="mt-2 text-2xl font-bold text-slate-900">
              Pre-built workflow blueprints
            </h2>
            <p className="mt-2 text-sm text-slate-500">
              Start from curated templates instead of building every DAG from zero.
            </p>
          </div>
          <div className="flex flex-col gap-3 sm:flex-row">
            <label className="relative min-w-[220px]">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
              <input
                value={templateQuery}
                onChange={(event) => onTemplateQueryChange(event.target.value)}
                placeholder="Search templates"
                className="w-full rounded-xl border border-slate-200 bg-slate-50 px-10 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              />
            </label>
            <select
              value={templateCategory}
              onChange={(event) => onTemplateCategoryChange(event.target.value)}
              className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
            >
              <option value="all">All categories</option>
              {categories.map((category) => (
                <option key={category} value={category}>
                  {category}
                </option>
              ))}
            </select>
          </div>
        </div>

        <WorkflowTemplateGrid
          templates={filteredTemplates}
          selectedTemplateId={selectedTemplateId}
          onPreview={onSelectTemplate}
          onApply={onApplyTemplate}
        />
      </section>

      <WorkflowTemplatePreview template={selectedTemplate} onApply={onApplyTemplate} />
    </div>
  );
}
