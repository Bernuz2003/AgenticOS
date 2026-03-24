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

export const workloadOptions: Array<{ value: DraftWorkload; label: string }> = [
  { value: "", label: "Default routing" },
  { value: "general", label: "General" },
  { value: "fast", label: "Fast" },
  { value: "code", label: "Code" },
  { value: "reasoning", label: "Reasoning" },
];

export const backendOptions: Array<{ value: DraftBackendClass; label: string }> = [
  { value: "", label: "Auto target" },
  { value: "resident_local", label: "Resident local" },
  { value: "remote_stateless", label: "Remote stateless" },
];

export const contextOptions: Array<{ value: DraftContextStrategy; label: string }> = [
  { value: "", label: "Kernel default" },
  { value: "sliding_window", label: "Sliding window" },
  { value: "summarize", label: "Summarize" },
  { value: "retrieve", label: "Retrieve" },
];

export function createTask(index: number): DraftTask {
  return {
    id: `task_${index}`,
    role: "",
    prompt: "",
    depsText: "",
    workload: "",
    backendClass: "",
    contextStrategy: "",
  };
}

export function initialTasks(): DraftTask[] {
  return [
    {
      id: "research",
      role: "Analyst",
      prompt: "Analyze the problem and extract the key facts, risks and constraints.",
      depsText: "",
      workload: "reasoning",
      backendClass: "",
      contextStrategy: "",
    },
    {
      id: "deliver",
      role: "Composer",
      prompt: "Produce the final deliverable using the upstream findings.",
      depsText: "research",
      workload: "general",
      backendClass: "",
      contextStrategy: "",
    },
  ];
}

export function splitDeps(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function toDateTimeLocalValue(timestampMs: number): string {
  const date = new Date(timestampMs);
  const pad = (value: number) => value.toString().padStart(2, "0");
  return [
    date.getFullYear(),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
  ].join("-")
    .concat("T")
    .concat([pad(date.getHours()), pad(date.getMinutes())].join(":"));
}

export function initialSchedulerDraft(): SchedulerDraft {
  const now = Date.now();
  return {
    name: "workflow_job",
    triggerKind: "interval",
    atLocal: toDateTimeLocalValue(now + 15 * 60 * 1000),
    intervalSeconds: "900",
    startsAtLocal: "",
    cronExpression: "*/30 * * * *",
    timeoutSeconds: "900",
    maxRetries: "1",
    backoffSeconds: "30",
    enabled: true,
  };
}

export function buildWorkflowPayload(
  failurePolicy: FailurePolicy,
  tasks: DraftTask[],
): WorkflowPayload {
  if (tasks.length === 0) {
    throw new Error("A workflow requires at least one task.");
  }

  const seen = new Set<string>();
  const normalizedTasks = tasks.map((task, index) => {
    const id = task.id.trim();
    const prompt = task.prompt.trim();
    if (!id) {
      throw new Error(`Task ${index + 1} is missing an id.`);
    }
    if (!prompt) {
      throw new Error(`Task ${id} is missing a prompt.`);
    }
    if (seen.has(id)) {
      throw new Error(`Duplicate task id '${id}'.`);
    }
    seen.add(id);

    const deps = splitDeps(task.depsText);
    if (deps.includes(id)) {
      throw new Error(`Task ${id} cannot depend on itself.`);
    }
    return {
      id,
      role: task.role.trim() || undefined,
      prompt,
      deps,
      workload: task.workload || undefined,
      backend_class: task.backendClass || undefined,
      context_strategy: task.contextStrategy || undefined,
    };
  });

  return {
    failure_policy: failurePolicy,
    tasks: normalizedTasks,
  };
}

export function buildSchedulePayload(
  failurePolicy: FailurePolicy,
  tasks: DraftTask[],
  schedulerDraft: SchedulerDraft,
) {
  const name = schedulerDraft.name.trim();
  if (!name) {
    throw new Error("Scheduled jobs require a name.");
  }

  let trigger: Record<string, unknown>;
  switch (schedulerDraft.triggerKind) {
    case "at": {
      const atMs = Date.parse(schedulerDraft.atLocal);
      if (!Number.isFinite(atMs)) {
        throw new Error("Select a valid run time for the one-shot trigger.");
      }
      trigger = {
        kind: "at",
        at_ms: atMs,
      };
      break;
    }
    case "interval": {
      const everySeconds = Number.parseInt(schedulerDraft.intervalSeconds, 10);
      if (!Number.isFinite(everySeconds) || everySeconds <= 0) {
        throw new Error("Interval triggers require a positive number of seconds.");
      }
      const startsAtMs = schedulerDraft.startsAtLocal
        ? Date.parse(schedulerDraft.startsAtLocal)
        : null;
      if (schedulerDraft.startsAtLocal && !Number.isFinite(startsAtMs)) {
        throw new Error("Select a valid scheduler anchor time.");
      }
      trigger = {
        kind: "interval",
        every_ms: everySeconds * 1000,
        starts_at_ms: startsAtMs,
      };
      break;
    }
    case "cron": {
      const expression = schedulerDraft.cronExpression.trim();
      if (!expression) {
        throw new Error("Cron triggers require an expression.");
      }
      trigger = {
        kind: "cron",
        expression,
      };
      break;
    }
    default:
      throw new Error("Unsupported scheduler trigger.");
  }

  const timeoutSeconds = Number.parseInt(schedulerDraft.timeoutSeconds, 10);
  const maxRetries = Number.parseInt(schedulerDraft.maxRetries, 10);
  const backoffSeconds = Number.parseInt(schedulerDraft.backoffSeconds, 10);

  return {
    name,
    workflow: buildWorkflowPayload(failurePolicy, tasks),
    trigger,
    timeout_ms:
      Number.isFinite(timeoutSeconds) && timeoutSeconds > 0
        ? timeoutSeconds * 1000
        : undefined,
    max_retries:
      Number.isFinite(maxRetries) && maxRetries >= 0 ? maxRetries : undefined,
    backoff_ms:
      Number.isFinite(backoffSeconds) && backoffSeconds > 0
        ? backoffSeconds * 1000
        : undefined,
    enabled: schedulerDraft.enabled,
  };
}
