import { analysisWorkflowTemplates } from "./analysis";
import { codingWorkflowTemplates } from "./coding";
import { comparisonWorkflowTemplates } from "./comparison";
import { type WorkflowTemplateDefinition } from "./catalog";
import { documentWorkflowTemplates } from "./documents";

export type { WorkflowTemplateDefinition } from "./catalog";
export { instantiateTemplateDraft } from "./catalog";

export const workflowTemplates: WorkflowTemplateDefinition[] = [
  ...codingWorkflowTemplates,
  ...analysisWorkflowTemplates,
  ...documentWorkflowTemplates,
  ...comparisonWorkflowTemplates,
];

export function workflowTemplateCategories(): string[] {
  return Array.from(
    new Set(workflowTemplates.map((template) => template.category)),
  ).sort();
}
