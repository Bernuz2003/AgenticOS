import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowRight, Plus, Trash2, Waypoints } from "lucide-react";
import {
  fetchOrchestrationStatus,
  orchestrate,
  retryWorkflowTask,
  scheduleWorkflowJob,
  type OrchestrationStatus,
  type ScheduledJob,
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";

type FailurePolicy = "fail_fast" | "best_effort";
type DraftWorkload = "" | "general" | "fast" | "code" | "reasoning";
type DraftBackendClass = "" | "resident_local" | "remote_stateless";
type DraftContextStrategy = "" | "sliding_window" | "summarize" | "retrieve";
type JobTriggerKind = "at" | "interval" | "cron";

interface DraftTask {
  id: string;
  role: string;
  prompt: string;
  depsText: string;
  workload: DraftWorkload;
  backendClass: DraftBackendClass;
  contextStrategy: DraftContextStrategy;
  contextWindowSize: string;
}

interface SchedulerDraft {
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

const workloadOptions: Array<{ value: DraftWorkload; label: string }> = [
  { value: "", label: "Default routing" },
  { value: "general", label: "General" },
  { value: "fast", label: "Fast" },
  { value: "code", label: "Code" },
  { value: "reasoning", label: "Reasoning" },
];

const backendOptions: Array<{ value: DraftBackendClass; label: string }> = [
  { value: "", label: "Auto target" },
  { value: "resident_local", label: "Resident local" },
  { value: "remote_stateless", label: "Remote stateless" },
];

const contextOptions: Array<{ value: DraftContextStrategy; label: string }> = [
  { value: "", label: "Kernel default" },
  { value: "sliding_window", label: "Sliding window" },
  { value: "summarize", label: "Summarize" },
  { value: "retrieve", label: "Retrieve" },
];

function createTask(index: number): DraftTask {
  return {
    id: `task_${index}`,
    role: "",
    prompt: "",
    depsText: "",
    workload: "",
    backendClass: "",
    contextStrategy: "",
    contextWindowSize: "",
  };
}

function initialTasks(): DraftTask[] {
  return [
    {
      id: "research",
      role: "Analyst",
      prompt: "Analyze the problem and extract the key facts, risks and constraints.",
      depsText: "",
      workload: "reasoning",
      backendClass: "",
      contextStrategy: "",
      contextWindowSize: "",
    },
    {
      id: "deliver",
      role: "Composer",
      prompt: "Produce the final deliverable using the upstream findings.",
      depsText: "research",
      workload: "general",
      backendClass: "",
      contextStrategy: "",
      contextWindowSize: "",
    },
  ];
}

function toDateTimeLocalValue(timestampMs: number): string {
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

function initialSchedulerDraft(): SchedulerDraft {
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

function splitDeps(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function formatElapsed(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 1) {
    return "<1s";
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDurationMs(milliseconds: number): string {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) {
    return "0s";
  }
  const totalSeconds = Math.round(milliseconds / 1000);
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  if (totalSeconds < 3600) {
    return `${Math.floor(totalSeconds / 60)}m`;
  }
  return `${Math.floor(totalSeconds / 3600)}h ${Math.floor(
    (totalSeconds % 3600) / 60,
  )}m`;
}

function taskStatusTone(status: string): string {
  switch (status) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "failed":
      return "border-rose-200 bg-rose-50 text-rose-700";
    case "skipped":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

function scheduledJobStateTone(state: string): string {
  switch (state) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "retry_wait":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "disabled":
      return "border-slate-200 bg-slate-100 text-slate-600";
    default:
      return "border-indigo-200 bg-indigo-50 text-indigo-700";
  }
}

function formatRecentRunLabel(job: ScheduledJob): string {
  if (job.nextRunAtMs) {
    return `next ${formatTimestamp(job.nextRunAtMs)}`;
  }
  if (job.lastRunCompletedAtMs) {
    return `last ${formatTimestamp(job.lastRunCompletedAtMs)}`;
  }
  return "not scheduled";
}

export function WorkflowsPage() {
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const scheduledJobs = useSessionsStore((state) => state.scheduledJobs);
  const refreshLobby = useSessionsStore((state) => state.refresh);

  const [failurePolicy, setFailurePolicy] = useState<FailurePolicy>("fail_fast");
  const [tasks, setTasks] = useState<DraftTask[]>(() => initialTasks());
  const [schedulerDraft, setSchedulerDraft] = useState<SchedulerDraft>(() =>
    initialSchedulerDraft(),
  );
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [submittingMode, setSubmittingMode] = useState<
    "launch" | "schedule" | null
  >(null);
  const [retryingTaskKey, setRetryingTaskKey] = useState<string | null>(null);
  const [workflowDetails, setWorkflowDetails] = useState<
    Record<number, OrchestrationStatus>
  >({});

  useEffect(() => {
    if (orchestrations.length === 0) {
      setWorkflowDetails({});
      return;
    }

    let cancelled = false;
    const load = async () => {
      const results = await Promise.allSettled(
        orchestrations.map(async (workflow) => [
          workflow.orchestrationId,
          await fetchOrchestrationStatus(workflow.orchestrationId),
        ] as const),
      );

      if (cancelled) {
        return;
      }

      const nextDetails: Record<number, OrchestrationStatus> = {};
      let firstError: string | null = null;

      for (const result of results) {
        if (result.status === "fulfilled") {
          const [orchestrationId, detail] = result.value;
          nextDetails[orchestrationId] = detail;
        } else if (!firstError) {
          firstError =
            result.reason instanceof Error
              ? result.reason.message
              : "Failed to fetch workflow monitor status";
        }
      }

      setWorkflowDetails(nextDetails);
      setStatusError(firstError);
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [orchestrations]);

  function updateTask(index: number, patch: Partial<DraftTask>) {
    setTasks((current) =>
      current.map((task, taskIndex) =>
        taskIndex === index ? { ...task, ...patch } : task,
      ),
    );
  }

  function addTask() {
    setTasks((current) => [...current, createTask(current.length + 1)]);
  }

  function updateSchedulerDraft(patch: Partial<SchedulerDraft>) {
    setSchedulerDraft((current) => ({
      ...current,
      ...patch,
    }));
  }

  function removeTask(index: number) {
    setTasks((current) => current.filter((_, taskIndex) => taskIndex !== index));
  }

  function buildWorkflowPayload() {
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

      const contextWindowSize = Number.parseInt(task.contextWindowSize, 10);

      return {
        id,
        role: task.role.trim() || undefined,
        prompt,
        deps,
        workload: task.workload || undefined,
        backend_class: task.backendClass || undefined,
        context_strategy: task.contextStrategy || undefined,
        context_window_size:
          Number.isFinite(contextWindowSize) && contextWindowSize > 0
            ? contextWindowSize
            : undefined,
      };
    });

    return {
      failure_policy: failurePolicy,
      tasks: normalizedTasks,
    };
  }

  function buildSchedulePayload() {
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
      workflow: buildWorkflowPayload(),
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

  function resetDrafts() {
    setTasks(initialTasks());
    setFailurePolicy("fail_fast");
    setSchedulerDraft(initialSchedulerDraft());
  }

  async function handleLaunchWorkflow() {
    setSubmittingMode("launch");
    setSubmitError(null);
    try {
      const payload = buildWorkflowPayload();
      const result = await orchestrate(JSON.stringify(payload));
      await refreshLobby();
      resetDrafts();
      if (!workflowDetails[result.orchestrationId]) {
        const detail = await fetchOrchestrationStatus(result.orchestrationId);
        setWorkflowDetails((current) => ({
          ...current,
          [result.orchestrationId]: detail,
        }));
      }
    } catch (error) {
      setSubmitError(
        error instanceof Error ? error.message : "Failed to launch workflow",
      );
    } finally {
      setSubmittingMode(null);
    }
  }

  async function handleScheduleWorkflow() {
    setSubmittingMode("schedule");
    setSubmitError(null);
    try {
      const payload = buildSchedulePayload();
      await scheduleWorkflowJob(JSON.stringify(payload));
      await refreshLobby();
      resetDrafts();
    } catch (error) {
      setSubmitError(
        error instanceof Error ? error.message : "Failed to schedule workflow job",
      );
    } finally {
      setSubmittingMode(null);
    }
  }

  async function handleRetryTask(orchestrationId: number, taskId: string) {
    const taskKey = `${orchestrationId}:${taskId}`;
    setRetryingTaskKey(taskKey);
    setStatusError(null);
    try {
      await retryWorkflowTask(orchestrationId, taskId);
      await refreshLobby();
      const detail = await fetchOrchestrationStatus(orchestrationId);
      setWorkflowDetails((current) => ({
        ...current,
        [orchestrationId]: detail,
      }));
    } catch (error) {
      setStatusError(
        error instanceof Error ? error.message : "Failed to retry workflow task",
      );
    } finally {
      setRetryingTaskKey(null);
    }
  }

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="rounded-3xl border border-slate-200 bg-white px-8 py-7 shadow-sm">
        <div className="flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
          <div className="max-w-3xl">
            <div className="text-xs font-bold uppercase tracking-[0.25em] text-slate-400">
              Workflow Control Plane
            </div>
            <h1 className="mt-2 text-3xl font-bold tracking-tight text-slate-900">
              Workflows
            </h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              Workflow execution is now a first-class path, separate from chat.
              Define DAG tasks, launch them through the control plane and monitor
              runtime execution without routing orchestration through a normal
              conversation.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Link
              to="/sessions"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Go to Chats
            </Link>
            <Link
              to="/control-center"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Open Control Center
            </Link>
          </div>
        </div>
      </header>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]">
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                New Workflow
              </div>
              <h2 className="mt-2 text-xl font-bold text-slate-900">
                Structured Task Graph
              </h2>
              <p className="mt-2 text-sm text-slate-500">
                Define task role, prompt, dependencies, workload, runtime
                target and context policy without going through chat. Launch it
                immediately or persist it as a scheduler job with temporal triggers.
              </p>
            </div>
            <div className="w-52">
              <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                Failure Policy
              </label>
              <select
                value={failurePolicy}
                onChange={(event) =>
                  setFailurePolicy(event.target.value as FailurePolicy)
                }
                className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              >
                <option value="fail_fast">Fail fast</option>
                <option value="best_effort">Best effort</option>
              </select>
            </div>
          </div>

          <div className="mt-6 space-y-4">
            {tasks.map((task, index) => (
              <div
                key={`${task.id}-${index}`}
                className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
              >
                <div className="mb-4 flex items-center justify-between gap-3">
                  <div>
                    <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                      Task {index + 1}
                    </div>
                    <div className="mt-1 text-sm font-semibold text-slate-900">
                      {task.id || `task_${index + 1}`}
                    </div>
                  </div>
                  <button
                    onClick={() => removeTask(index)}
                    disabled={tasks.length === 1}
                    className="rounded-xl border border-slate-200 bg-white p-2 text-slate-500 hover:text-rose-600 disabled:opacity-40"
                    title="Remove task"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Task ID
                    </label>
                    <input
                      value={task.id}
                      onChange={(event) =>
                        updateTask(index, { id: event.target.value })
                      }
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Role
                    </label>
                    <input
                      value={task.role}
                      onChange={(event) =>
                        updateTask(index, { role: event.target.value })
                      }
                      placeholder="Analyst, Reviewer, Synthesizer..."
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Workload
                    </label>
                    <select
                      value={task.workload}
                      onChange={(event) =>
                        updateTask(index, {
                          workload: event.target.value as DraftWorkload,
                        })
                      }
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    >
                      {workloadOptions.map((option) => (
                        <option key={option.value || "default"} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Runtime Target
                    </label>
                    <select
                      value={task.backendClass}
                      onChange={(event) =>
                        updateTask(index, {
                          backendClass: event.target.value as DraftBackendClass,
                        })
                      }
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    >
                      {backendOptions.map((option) => (
                        <option key={option.value || "auto"} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Context Strategy
                    </label>
                    <select
                      value={task.contextStrategy}
                      onChange={(event) =>
                        updateTask(index, {
                          contextStrategy:
                            event.target.value as DraftContextStrategy,
                        })
                      }
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    >
                      {contextOptions.map((option) => (
                        <option
                          key={option.value || "kernel_default"}
                          value={option.value}
                        >
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Context Window
                    </label>
                    <input
                      value={task.contextWindowSize}
                      onChange={(event) =>
                        updateTask(index, {
                          contextWindowSize: event.target.value,
                        })
                      }
                      placeholder="Optional token budget"
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>
                </div>

                <div className="mt-4">
                  <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                    Dependencies
                  </label>
                  <input
                    value={task.depsText}
                    onChange={(event) =>
                      updateTask(index, { depsText: event.target.value })
                    }
                    placeholder="Comma-separated task ids"
                    className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                </div>

                <div className="mt-4">
                  <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                    Prompt
                  </label>
                  <textarea
                    value={task.prompt}
                    onChange={(event) =>
                      updateTask(index, { prompt: event.target.value })
                    }
                    className="min-h-[130px] w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm leading-relaxed text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                </div>
              </div>
            ))}
          </div>

          <div className="mt-5 flex items-center justify-between gap-3">
            <button
              onClick={addTask}
              className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              <Plus className="h-4 w-4" />
              Add Task
            </button>
            <div className="text-xs text-slate-400">
              Root tasks are the ones without dependencies.
            </div>
          </div>

          <div className="mt-6 rounded-2xl border border-slate-200 bg-slate-50 p-5">
            <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
              <div>
                <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                  Scheduler Trigger
                </div>
                <div className="mt-1 text-sm font-semibold text-slate-900">
                  Persisted background execution
                </div>
                <p className="mt-2 max-w-2xl text-sm text-slate-500">
                  The same workflow graph can run now through the control plane or
                  become a durable background job with `at`, `interval` or cron-like
                  triggers, retry/backoff and timeout enforcement.
                </p>
              </div>
              <label className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs font-semibold uppercase tracking-wider text-slate-600">
                <input
                  type="checkbox"
                  checked={schedulerDraft.enabled}
                  onChange={(event) =>
                    updateSchedulerDraft({ enabled: event.target.checked })
                  }
                  className="h-4 w-4 rounded border-slate-300 text-indigo-600 focus:ring-indigo-500"
                />
                Enabled
              </label>
            </div>

            <div className="mt-5 grid gap-4 md:grid-cols-2">
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Job Name
                </label>
                <input
                  value={schedulerDraft.name}
                  onChange={(event) =>
                    updateSchedulerDraft({ name: event.target.value })
                  }
                  placeholder="nightly_sync"
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
              </div>
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Trigger Kind
                </label>
                <select
                  value={schedulerDraft.triggerKind}
                  onChange={(event) =>
                    updateSchedulerDraft({
                      triggerKind: event.target.value as JobTriggerKind,
                    })
                  }
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                >
                  <option value="interval">Interval</option>
                  <option value="at">Run once at</option>
                  <option value="cron">Cron-like</option>
                </select>
              </div>
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Timeout
                </label>
                <input
                  value={schedulerDraft.timeoutSeconds}
                  onChange={(event) =>
                    updateSchedulerDraft({ timeoutSeconds: event.target.value })
                  }
                  placeholder="Seconds before the job times out"
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
              </div>
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Max Retries
                </label>
                <input
                  value={schedulerDraft.maxRetries}
                  onChange={(event) =>
                    updateSchedulerDraft({ maxRetries: event.target.value })
                  }
                  placeholder="0, 1, 2..."
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
              </div>
              <div>
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Backoff
                </label>
                <input
                  value={schedulerDraft.backoffSeconds}
                  onChange={(event) =>
                    updateSchedulerDraft({ backoffSeconds: event.target.value })
                  }
                  placeholder="Seconds before retry"
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
              </div>
            </div>

            {schedulerDraft.triggerKind === "at" && (
              <div className="mt-4">
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Run At
                </label>
                <input
                  type="datetime-local"
                  value={schedulerDraft.atLocal}
                  onChange={(event) =>
                    updateSchedulerDraft({ atLocal: event.target.value })
                  }
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
              </div>
            )}

            {schedulerDraft.triggerKind === "interval" && (
              <div className="mt-4 grid gap-4 md:grid-cols-2">
                <div>
                  <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                    Every
                  </label>
                  <input
                    value={schedulerDraft.intervalSeconds}
                    onChange={(event) =>
                      updateSchedulerDraft({ intervalSeconds: event.target.value })
                    }
                    placeholder="Seconds between runs"
                    className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                </div>
                <div>
                  <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                    Starts At
                  </label>
                  <input
                    type="datetime-local"
                    value={schedulerDraft.startsAtLocal}
                    onChange={(event) =>
                      updateSchedulerDraft({ startsAtLocal: event.target.value })
                    }
                    className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                  <div className="mt-2 text-xs text-slate-400">
                    Leave empty to anchor the interval at creation time.
                  </div>
                </div>
              </div>
            )}

            {schedulerDraft.triggerKind === "cron" && (
              <div className="mt-4">
                <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                  Cron Expression
                </label>
                <input
                  value={schedulerDraft.cronExpression}
                  onChange={(event) =>
                    updateSchedulerDraft({ cronExpression: event.target.value })
                  }
                  placeholder="*/30 * * * *"
                  className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                />
                <div className="mt-2 text-xs text-slate-400">
                  Five fields: minute hour day-of-month month day-of-week.
                </div>
              </div>
            )}
          </div>

          <div className="mt-6 flex flex-wrap items-center justify-between gap-3">
            <div className="text-xs text-slate-400">
              Immediate launch creates a live orchestration now. Scheduler mode stores
              a durable job and lets the kernel trigger it later.
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <button
                onClick={handleScheduleWorkflow}
                disabled={submittingMode !== null}
                className="rounded-xl border border-indigo-200 bg-indigo-50 px-4 py-2.5 text-sm font-semibold text-indigo-700 hover:bg-indigo-100 disabled:opacity-40"
              >
                {submittingMode === "schedule" ? "Scheduling..." : "Schedule Job"}
              </button>
              <button
                onClick={handleLaunchWorkflow}
                disabled={submittingMode !== null}
                className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800 disabled:opacity-40"
              >
                <Waypoints className="h-4 w-4" />
                {submittingMode === "launch" ? "Launching..." : "Launch Workflow"}
              </button>
            </div>
          </div>

          {submitError && (
            <div className="mt-5 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
              {submitError}
            </div>
          )}
        </section>

        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Scheduler & Workflow Monitor
              </div>
              <h2 className="mt-2 text-xl font-bold text-slate-900">
                Scheduled jobs, retries and live task execution
              </h2>
              <p className="mt-2 text-sm text-slate-500">
                Observe durable job state, next trigger times, retry/backoff
                behavior, timeout envelopes and live orchestrations spawned by the
                scheduler or launched manually.
              </p>
            </div>
            <button
              onClick={() => void refreshLobby()}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Refresh
            </button>
          </div>

          <div className="mt-6 rounded-2xl border border-slate-200 bg-slate-50 p-5">
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div>
                <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                  Scheduled Jobs
                </div>
                <div className="mt-1 text-sm text-slate-500">
                  Persisted workflow jobs tracked by the kernel scheduler.
                </div>
              </div>
              <div className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                {scheduledJobs.length} jobs
              </div>
            </div>

            {scheduledJobs.length === 0 ? (
              <div className="mt-4 rounded-2xl border border-dashed border-slate-200 bg-white px-5 py-8 text-center text-sm text-slate-500">
                No scheduled jobs yet. Use the scheduler trigger panel to persist a
                background workflow.
              </div>
            ) : (
              <div className="mt-4 space-y-3">
                {scheduledJobs.map((job) => (
                  <div
                    key={job.jobId}
                    className="rounded-2xl border border-slate-200 bg-white p-4"
                  >
                    <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <div className="text-sm font-semibold text-slate-900">
                            {job.name}
                          </div>
                          <span
                            className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${scheduledJobStateTone(job.state)}`}
                          >
                            {job.state}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                            {job.triggerKind}
                          </span>
                          {!job.enabled && (
                            <span className="rounded-full border border-slate-200 bg-slate-100 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                              disabled
                            </span>
                          )}
                        </div>
                        <div className="mt-2 text-xs text-slate-500">
                          {job.triggerLabel}
                        </div>
                        <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            {formatRecentRunLabel(job)}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            timeout {formatDurationMs(job.timeoutMs)}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            backoff {formatDurationMs(job.backoffMs)}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            retries {job.maxRetries}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            consecutive failures {job.consecutiveFailures}
                          </span>
                          {job.activeOrchestrationId !== null && (
                            <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-indigo-700">
                              orchestration {job.activeOrchestrationId}
                            </span>
                          )}
                        </div>
                        {job.lastError && (
                          <div className="mt-3 rounded-xl border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
                            {job.lastError}
                          </div>
                        )}
                      </div>

                      <div className="w-full max-w-sm rounded-2xl border border-slate-200 bg-slate-50 p-3">
                        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                          Recent Runs
                        </div>
                        {job.recentRuns.length === 0 ? (
                          <div className="mt-2 text-xs text-slate-500">
                            No recorded attempts yet.
                          </div>
                        ) : (
                          <div className="mt-3 space-y-2">
                            {job.recentRuns.slice(0, 3).map((run) => (
                              <div
                                key={run.runId}
                                className="rounded-xl border border-slate-200 bg-white px-3 py-2"
                              >
                                <div className="flex flex-wrap items-center justify-between gap-2 text-[11px]">
                                  <div className="flex flex-wrap items-center gap-2">
                                    <span className="font-semibold text-slate-700">
                                      run {run.runId}
                                    </span>
                                    <span
                                      className={`rounded-full border px-2 py-0.5 font-semibold uppercase tracking-wider ${taskStatusTone(run.status)}`}
                                    >
                                      {run.status}
                                    </span>
                                  </div>
                                  <span className="text-slate-500">
                                    attempt {run.attempt}
                                  </span>
                                </div>
                                <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                                  <span className="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5">
                                    trigger {formatTimestamp(run.triggerAtMs)}
                                  </span>
                                  {run.orchestrationId !== null && (
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5">
                                      orchestration {run.orchestrationId}
                                    </span>
                                  )}
                                  {run.completedAtMs && (
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2 py-0.5">
                                      completed {formatTimestamp(run.completedAtMs)}
                                    </span>
                                  )}
                                </div>
                                {run.error && (
                                  <div className="mt-2 text-xs text-rose-700">
                                    {run.error}
                                  </div>
                                )}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          {statusError && (
            <div className="mt-5 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-700">
              {statusError}
            </div>
          )}

          <div className="mt-6 text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Live Orchestrations
          </div>

          {orchestrations.length === 0 ? (
            <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center">
              <div className="text-lg font-semibold text-slate-800">
                No active workflows
              </div>
              <div className="mt-2 text-sm text-slate-500">
                Launch a workflow from the builder to populate the monitor.
              </div>
            </div>
          ) : (
            <div className="mt-6 space-y-4">
              {orchestrations.map((workflow) => {
                const detail = workflowDetails[workflow.orchestrationId];
                return (
                  <div
                    key={workflow.orchestrationId}
                    className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
                  >
                    <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                      <div>
                        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                          Workflow {workflow.orchestrationId}
                        </div>
                        <div className="mt-2 flex flex-wrap items-center gap-2">
                          <span className="rounded-full border border-indigo-200 bg-indigo-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-indigo-700">
                            {workflow.policy}
                          </span>
                          <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                            elapsed {workflow.elapsedLabel}
                          </span>
                        </div>
                      </div>

                      <div className="grid grid-cols-5 gap-2 text-center text-xs">
                        <div className="rounded-xl border border-slate-200 bg-white px-3 py-2">
                          <div className="text-[10px] uppercase tracking-wider text-slate-400">
                            Total
                          </div>
                          <div className="mt-1 font-bold text-slate-900">
                            {workflow.total}
                          </div>
                        </div>
                        <div className="rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-2">
                          <div className="text-[10px] uppercase tracking-wider text-emerald-500">
                            Run
                          </div>
                          <div className="mt-1 font-bold text-emerald-700">
                            {workflow.running}
                          </div>
                        </div>
                        <div className="rounded-xl border border-sky-200 bg-sky-50 px-3 py-2">
                          <div className="text-[10px] uppercase tracking-wider text-sky-500">
                            Done
                          </div>
                          <div className="mt-1 font-bold text-sky-700">
                            {workflow.completed}
                          </div>
                        </div>
                        <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2">
                          <div className="text-[10px] uppercase tracking-wider text-amber-500">
                            Wait
                          </div>
                          <div className="mt-1 font-bold text-amber-700">
                            {workflow.pending}
                          </div>
                        </div>
                        <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-2">
                          <div className="text-[10px] uppercase tracking-wider text-rose-500">
                            Fail
                          </div>
                          <div className="mt-1 font-bold text-rose-700">
                            {workflow.failed}
                          </div>
                        </div>
                      </div>
                    </div>

                    {!detail ? (
                      <div className="mt-4 text-sm text-slate-500">
                        Loading workflow details...
                      </div>
                    ) : (
                      <div className="mt-5 space-y-3">
                        <div className="flex flex-wrap items-center gap-3 text-xs text-slate-500">
                          <span>
                            Stored output chars:{" "}
                            <strong className="text-slate-700">
                              {detail.outputCharsStored.toLocaleString()}
                            </strong>
                          </span>
                          <span>
                            Truncations:{" "}
                            <strong className="text-slate-700">
                              {detail.truncations}
                            </strong>
                          </span>
                          <span>
                            Elapsed:{" "}
                            <strong className="text-slate-700">
                              {formatElapsed(detail.elapsedSecs)}
                            </strong>
                          </span>
                        </div>

                        {detail.tasks.map((task) => {
                          const activeAttempt = task.attempts.find(
                            (attempt) =>
                              attempt.attempt === task.currentAttempt &&
                              attempt.sessionId,
                          );
                          const latestAttemptWithSession = task.attempts.find(
                            (attempt) => attempt.sessionId,
                          );
                          const sessionId =
                            activeAttempt?.sessionId ?? latestAttemptWithSession?.sessionId ?? null;
                          const retryTaskKey = `${detail.orchestrationId}:${task.task}`;
                          const canRetry =
                            task.status !== "running" &&
                            (task.attempts.length > 0 || task.status === "skipped");

                          return (
                            <div
                              key={`${detail.orchestrationId}-${task.task}`}
                              className="rounded-2xl border border-slate-200 bg-white p-4"
                            >
                              <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
                                <div className="min-w-0 flex-1">
                                  <div className="flex flex-wrap items-center gap-2">
                                    <div className="text-sm font-semibold text-slate-900">
                                      {task.task}
                                    </div>
                                    <span
                                      className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskStatusTone(task.status)}`}
                                    >
                                      {task.status}
                                    </span>
                                    {task.role && (
                                      <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                                        {task.role}
                                      </span>
                                    )}
                                  </div>
                                  <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                                      workload {task.workload ?? "default"}
                                    </span>
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                                      target {task.backendClass ?? "auto"}
                                    </span>
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                                      context {task.contextStrategy ?? "kernel_default"}
                                    </span>
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                                      deps {task.deps.length === 0 ? "root" : task.deps.join(", ")}
                                    </span>
                                    {task.currentAttempt !== null && (
                                      <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                                        attempt {task.currentAttempt}
                                      </span>
                                    )}
                                  </div>
                                  {task.error && (
                                    <div className="mt-3 rounded-xl border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
                                      {task.error}
                                    </div>
                                  )}
                                  {task.inputArtifacts.length > 0 && (
                                    <div className="mt-4">
                                      <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                                        Input Artifacts
                                      </div>
                                      <div className="mt-2 flex flex-wrap gap-2">
                                        {task.inputArtifacts.map((artifact) => (
                                          <span
                                            key={artifact.artifactId}
                                            className="rounded-full border border-emerald-200 bg-emerald-50 px-2.5 py-1 text-[11px] font-medium text-emerald-700"
                                          >
                                            {artifact.task}#{artifact.attempt}
                                          </span>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                  {(task.latestOutputPreview || task.outputArtifacts.length > 0) && (
                                    <div className="mt-4 grid gap-3 xl:grid-cols-[minmax(0,1.15fr)_minmax(240px,0.85fr)]">
                                      <div className="rounded-2xl border border-slate-200 bg-slate-50 p-3">
                                        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                                          Latest Output
                                        </div>
                                        <div className="mt-2 whitespace-pre-wrap break-words text-xs leading-6 text-slate-700">
                                          {task.latestOutputPreview ?? "No output captured yet."}
                                        </div>
                                      </div>
                                      <div className="rounded-2xl border border-slate-200 bg-slate-50 p-3">
                                        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                                          Artifacts
                                        </div>
                                        {task.outputArtifacts.length === 0 ? (
                                          <div className="mt-2 text-xs text-slate-500">
                                            No persisted artifacts yet.
                                          </div>
                                        ) : (
                                          <div className="mt-2 space-y-2">
                                            {task.outputArtifacts.map((artifact) => (
                                              <div
                                                key={artifact.artifactId}
                                                className="rounded-xl border border-slate-200 bg-white px-3 py-2"
                                              >
                                                <div className="flex items-center justify-between gap-3 text-[11px]">
                                                  <span className="font-semibold text-slate-700">
                                                    {artifact.label}
                                                  </span>
                                                  <span className="text-slate-500">
                                                    {formatBytes(artifact.bytes)}
                                                  </span>
                                                </div>
                                                <div className="mt-1 text-[11px] text-slate-500">
                                                  {artifact.kind} · attempt {artifact.attempt}
                                                </div>
                                                <div className="mt-2 whitespace-pre-wrap break-words text-xs leading-6 text-slate-600">
                                                  {artifact.preview || "Empty artifact"}
                                                </div>
                                              </div>
                                            ))}
                                          </div>
                                        )}
                                      </div>
                                    </div>
                                  )}
                                  {task.attempts.length > 0 && (
                                    <div className="mt-4">
                                      <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                                        Attempt History
                                      </div>
                                      <div className="mt-2 space-y-2">
                                        {task.attempts.map((attempt) => (
                                          <div
                                            key={`${task.task}-attempt-${attempt.attempt}`}
                                            className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2"
                                          >
                                            <div className="flex flex-wrap items-center justify-between gap-3">
                                              <div className="flex flex-wrap items-center gap-2 text-[11px]">
                                                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 font-semibold text-slate-700">
                                                  attempt {attempt.attempt}
                                                </span>
                                                <span
                                                  className={`rounded-full border px-2.5 py-1 font-semibold uppercase tracking-wider ${taskStatusTone(attempt.status)}`}
                                                >
                                                  {attempt.status}
                                                </span>
                                                <span className="text-slate-500">
                                                  {formatTimestamp(attempt.startedAtMs)}
                                                </span>
                                              </div>
                                              {attempt.sessionId && (
                                                <Link
                                                  to={`/workspace/${attempt.sessionId}`}
                                                  className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-3 py-2 text-[11px] font-semibold text-white hover:bg-slate-800"
                                                >
                                                  Open attempt
                                                  <ArrowRight className="h-3.5 w-3.5" />
                                                </Link>
                                              )}
                                            </div>
                                            <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                                              <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                                                chars {attempt.outputChars.toLocaleString()}
                                              </span>
                                              {attempt.primaryArtifactId && (
                                                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                                                  artifact saved
                                                </span>
                                              )}
                                              {attempt.completedAtMs && (
                                                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                                                  completed {formatTimestamp(attempt.completedAtMs)}
                                                </span>
                                              )}
                                            </div>
                                            {attempt.outputPreview && (
                                              <div className="mt-2 whitespace-pre-wrap break-words text-xs leading-6 text-slate-600">
                                                {attempt.outputPreview}
                                              </div>
                                            )}
                                          </div>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                </div>

                                <div className="flex flex-wrap items-center gap-2">
                                  {task.pid !== null && (
                                    <span className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-semibold text-slate-600">
                                      PID {task.pid}
                                    </span>
                                  )}
                                  {sessionId && (
                                    <Link
                                      to={`/workspace/${sessionId}`}
                                      className="inline-flex items-center gap-2 rounded-xl bg-indigo-50 px-3 py-2 text-xs font-semibold text-indigo-700 hover:bg-indigo-100"
                                    >
                                      Open task workspace
                                      <ArrowRight className="h-3.5 w-3.5" />
                                    </Link>
                                  )}
                                  <button
                                    onClick={() =>
                                      void handleRetryTask(detail.orchestrationId, task.task)
                                    }
                                    disabled={!canRetry || retryingTaskKey === retryTaskKey}
                                    className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-40"
                                  >
                                    {retryingTaskKey === retryTaskKey
                                      ? "Retrying..."
                                      : "Retry Task"}
                                  </button>
                                </div>
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
