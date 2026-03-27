import type {
  DraftTask,
  FailurePolicy,
  SchedulerDraft,
  WorkflowPayload,
} from "./index";
import { validateDraftTask, validateScheduledJobName } from "./validation";

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
    const { id, prompt, deps } = validateDraftTask(task, index, seen);
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
  const name = validateScheduledJobName(schedulerDraft.name);

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
