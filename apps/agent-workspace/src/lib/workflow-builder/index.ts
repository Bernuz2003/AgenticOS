export type FailurePolicy = "fail_fast" | "best_effort";
export type DraftWorkload = "" | "general" | "fast" | "code" | "reasoning";
export type DraftBackendClass = "" | "resident_local" | "remote_stateless";
export type DraftContextStrategy = "" | "sliding_window" | "summarize" | "retrieve";
export type JobTriggerKind = "at" | "interval" | "cron";

export interface DraftTask {
  id: string;
  role: string;
  prompt: string;
  depsText: string;
  workload: DraftWorkload;
  backendClass: DraftBackendClass;
  contextStrategy: DraftContextStrategy;
}

export interface SchedulerDraft {
  name: string;
  triggerKind: JobTriggerKind;
  atLocal: string;
  intervalSeconds: string;
  startsAtLocal: string;
  cronExpression: string;
  timeoutSeconds: string;
  maxRetries: string;
  backoffSeconds: string;
  enabled: boolean;
}

export interface WorkflowPayloadTask {
  id: string;
  role?: string;
  prompt: string;
  deps: string[];
  workload?: Exclude<DraftWorkload, "">;
  backend_class?: Exclude<DraftBackendClass, "">;
  context_strategy?: Exclude<DraftContextStrategy, "">;
}

export interface WorkflowPayload {
  failure_policy: FailurePolicy;
  tasks: WorkflowPayloadTask[];
}

export {
  buildSchedulePayload,
  buildWorkflowPayload,
  createTask,
  initialSchedulerDraft,
  initialTasks,
  toDateTimeLocalValue,
} from "./payload";
export {
  backendOptions,
  contextOptions,
  splitDeps,
  validateDraftTask,
  validateScheduledJobName,
  workloadOptions,
} from "./validation";
