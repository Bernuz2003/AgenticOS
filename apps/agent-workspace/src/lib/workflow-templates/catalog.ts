import {
  initialSchedulerDraft,
  type DraftBackendClass,
  type DraftContextStrategy,
  type DraftTask,
  type DraftWorkload,
  type FailurePolicy,
  type SchedulerDraft,
} from "../workflow-builder";

export interface WorkflowTemplateTask {
  id: string;
  role: string;
  prompt: string;
  deps?: string[];
  workload?: Exclude<DraftWorkload, "">;
  backendClass?: Exclude<DraftBackendClass, "">;
  contextStrategy?: Exclude<DraftContextStrategy, "">;
}

export interface WorkflowTemplateDefinition {
  id: string;
  name: string;
  category: string;
  description: string;
  summary: string;
  tags: string[];
  recommendedRuntime: string;
  failurePolicy: FailurePolicy;
  tasks: WorkflowTemplateTask[];
  schedulerPreset?: Partial<SchedulerDraft>;
}

interface PromptSpec {
  objective: string;
  inputs: string[];
  outputLines: string[];
  constraints?: string[];
}

const BASE_RESULT_CONSTRAINTS = [
  "Do the work directly; do not narrate that you will do it.",
  "Do not include planning chatter, repeated tool intentions or transcript-style commentary in the final result.",
  "If tools are needed, use them, then return only the durable result artifact.",
  "Keep the result information-dense and easy for the next task to consume.",
];

function bulletList(items: string[]): string {
  return items.map((item) => `- ${item}`).join("\n");
}

export function buildStructuredTaskPrompt(spec: PromptSpec): string {
  return [
    "[Objective]",
    spec.objective,
    "",
    "[Input Contract]",
    bulletList(spec.inputs),
    "",
    "[Output Contract]",
    ...spec.outputLines,
    "",
    "[Behavior Constraints]",
    bulletList([...BASE_RESULT_CONSTRAINTS, ...(spec.constraints ?? [])]),
  ].join("\n");
}

export function instantiateTemplateDraft(
  template: WorkflowTemplateDefinition,
): {
  failurePolicy: FailurePolicy;
  tasks: DraftTask[];
  schedulerDraft: SchedulerDraft;
} {
  const schedulerDraft = {
    ...initialSchedulerDraft(),
    ...template.schedulerPreset,
  };

  return {
    failurePolicy: template.failurePolicy,
    schedulerDraft,
    tasks: template.tasks.map((task) => ({
      id: task.id,
      role: task.role,
      prompt: task.prompt,
      depsText: (task.deps ?? []).join(", "),
      workload: task.workload ?? "",
      backendClass: task.backendClass ?? "",
      contextStrategy: task.contextStrategy ?? "",
    })),
  };
}
