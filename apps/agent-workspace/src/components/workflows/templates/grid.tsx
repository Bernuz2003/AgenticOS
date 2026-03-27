import type { WorkflowTemplateDefinition } from "../../../lib/workflow-templates";
import { WorkflowTemplateCard } from "./card";

interface WorkflowTemplateGridProps {
  templates: WorkflowTemplateDefinition[];
  selectedTemplateId: string;
  onPreview: (templateId: string) => void;
  onApply: (templateId: string) => void;
}

export function WorkflowTemplateGrid({
  templates,
  selectedTemplateId,
  onPreview,
  onApply,
}: WorkflowTemplateGridProps) {
  return (
    <div className="mt-6 grid gap-4 lg:grid-cols-2">
      {templates.map((template) => (
        <WorkflowTemplateCard
          key={template.id}
          template={template}
          selected={template.id === selectedTemplateId}
          onPreview={onPreview}
          onApply={onApply}
        />
      ))}
    </div>
  );
}
