import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  ArrowRight,
  CalendarClock,
  Layers3,
  Search,
  Sparkles,
  Waypoints,
} from "lucide-react";
import { orchestrate, scheduleWorkflowJob } from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";
import { WorkflowCanvasBuilder } from "../components/workflows/workflow-canvas-builder";
import {
  backendOptions,
  buildSchedulePayload,
  buildWorkflowPayload,
  contextOptions,
  createTask,
  initialSchedulerDraft,
  initialTasks,
  type DraftBackendClass,
  type DraftContextStrategy,
  type DraftTask,
  type DraftWorkload,
  type FailurePolicy,
  type JobTriggerKind,
  type SchedulerDraft,
  workloadOptions,
} from "../lib/workflow-builder";
import { taskDisplayId, validateWorkflowDraft } from "../lib/workflow-graph";
import {
  instantiateTemplateDraft,
  workflowTemplateCategories,
  workflowTemplates,
} from "../lib/workflow-templates";

function formatDateTimeLocal(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

function formatSchedulerSummary(schedulerDraft: SchedulerDraft): string {
  switch (schedulerDraft.triggerKind) {
    case "at":
      return `One-shot at ${formatDateTimeLocal(Date.parse(schedulerDraft.atLocal))}`;
    case "cron":
      return `Cron ${schedulerDraft.cronExpression || "not set"}`;
    case "interval":
    default:
      return `Every ${schedulerDraft.intervalSeconds || "?"}s`;
  }
}

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

export function WorkflowsPage() {
  const navigate = useNavigate();
  const refreshLobby = useSessionsStore((state) => state.refresh);
  const [view, setView] = useState<"templates" | "builder">("templates");
  const [templateQuery, setTemplateQuery] = useState("");
  const [templateCategory, setTemplateCategory] = useState("all");
  const [selectedTemplateId, setSelectedTemplateId] = useState(
    workflowTemplates[0]?.id ?? "",
  );
  const [draftSourceTemplateId, setDraftSourceTemplateId] = useState<string | null>(null);
  const [failurePolicy, setFailurePolicy] = useState<FailurePolicy>("fail_fast");
  const [tasks, setTasks] = useState<DraftTask[]>(() => initialTasks());
  const [selectedTaskIndex, setSelectedTaskIndex] = useState<number | null>(0);
  const [schedulerDraft, setSchedulerDraft] = useState<SchedulerDraft>(() =>
    initialSchedulerDraft(),
  );
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [submittingMode, setSubmittingMode] = useState<"launch" | "schedule" | null>(null);

  const categories = useMemo(() => workflowTemplateCategories(), []);
  const filteredTemplates = useMemo(() => {
    const query = templateQuery.trim().toLowerCase();
    return workflowTemplates.filter((template) => {
      const matchesCategory =
        templateCategory === "all" || template.category === templateCategory;
      const matchesQuery =
        !query ||
        [
          template.name,
          template.summary,
          template.description,
          template.category,
          ...template.tags,
        ]
          .join(" ")
          .toLowerCase()
          .includes(query);
      return matchesCategory && matchesQuery;
    });
  }, [templateCategory, templateQuery]);
  const selectedTemplate =
    workflowTemplates.find((template) => template.id === selectedTemplateId) ??
    filteredTemplates[0] ??
    workflowTemplates[0] ??
    null;
  const rootTasks = tasks.filter((task) => task.depsText.trim() === "");
  const selectedTask =
    (selectedTaskIndex !== null ? tasks[selectedTaskIndex] : null) ?? tasks[0] ?? null;
  const validation = useMemo(() => validateWorkflowDraft(tasks), [tasks]);

  useEffect(() => {
    if (tasks.length === 0) {
      setSelectedTaskIndex(null);
      return;
    }
    if (
      selectedTaskIndex === null ||
      selectedTaskIndex < 0 ||
      selectedTaskIndex >= tasks.length
    ) {
      setSelectedTaskIndex(0);
    }
  }, [selectedTaskIndex, tasks.length]);

  function updateTask(index: number, patch: Partial<DraftTask>) {
    setTasks((current) =>
      current.map((task, taskIndex) =>
        taskIndex === index ? { ...task, ...patch } : task,
      ),
    );
  }

  function addTask() {
    setTasks((current) => {
      const next = [...current, createTask(current.length + 1)];
      setSelectedTaskIndex(next.length - 1);
      return next;
    });
  }

  function removeTask(index: number) {
    setTasks((current) => current.filter((_, taskIndex) => taskIndex !== index));
    setSelectedTaskIndex((current) => {
      if (current === null) {
        return 0;
      }
      if (current === index) {
        return Math.max(0, index - 1);
      }
      if (current > index) {
        return current - 1;
      }
      return current;
    });
  }

  function resetBuilder() {
    setDraftSourceTemplateId(null);
    setFailurePolicy("fail_fast");
    setTasks(initialTasks());
    setSelectedTaskIndex(0);
    setSchedulerDraft(initialSchedulerDraft());
    setSubmitError(null);
  }

  function applyTemplate(templateId: string) {
    const template = workflowTemplates.find((candidate) => candidate.id === templateId);
    if (!template) {
      return;
    }
    const draft = instantiateTemplateDraft(template);
    setSelectedTemplateId(template.id);
    setDraftSourceTemplateId(template.id);
    setFailurePolicy(draft.failurePolicy);
    setTasks(draft.tasks);
    setSelectedTaskIndex(0);
    setSchedulerDraft(draft.schedulerDraft);
    setSubmitError(null);
    setView("builder");
  }

  async function handleLaunchWorkflow() {
    setSubmittingMode("launch");
    setSubmitError(null);
    try {
      const payload = buildWorkflowPayload(failurePolicy, tasks);
      const result = await orchestrate(JSON.stringify(payload));
      await refreshLobby();
      navigate(`/workflow-runs/${result.orchestrationId}`);
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
      const payload = buildSchedulePayload(failurePolicy, tasks, schedulerDraft);
      await scheduleWorkflowJob(JSON.stringify(payload));
      await refreshLobby();
      navigate("/jobs");
    } catch (error) {
      setSubmitError(
        error instanceof Error ? error.message : "Failed to schedule workflow",
      );
    } finally {
      setSubmittingMode(null);
    }
  }

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="overflow-hidden rounded-[32px] border border-slate-200 bg-white shadow-sm">
        <div className="bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.14),_transparent_48%),linear-gradient(135deg,_rgba(248,250,252,1),_rgba(255,255,255,0.96))] px-8 py-8">
          <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
            <div className="max-w-3xl">
              <div className="text-xs font-bold uppercase tracking-[0.28em] text-slate-400">
                Workflow Studio
              </div>
              <h1 className="mt-3 text-3xl font-bold tracking-tight text-slate-900">
                Templates and builder stay here. Execution moves to Jobs.
              </h1>
              <p className="mt-3 max-w-2xl text-sm leading-6 text-slate-600">
                `Chats` remain conversational. `Workflows` is now the design surface:
                choose a template, customize the DAG and optionally attach a scheduler.
                Live runs and scheduled jobs are monitored in a dedicated runtime view.
              </p>
            </div>
            <div className="flex flex-wrap gap-3">
              <Link
                to="/sessions"
                className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
              >
                Go to Chats
              </Link>
              <Link
                to="/jobs"
                className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
              >
                Open Jobs
                <ArrowRight className="h-4 w-4" />
              </Link>
            </div>
          </div>

          <div className="mt-8 inline-flex rounded-2xl border border-slate-200 bg-white p-1 shadow-sm">
            <button
              type="button"
              onClick={() => setView("templates")}
              className={`rounded-xl px-4 py-2 text-sm font-semibold transition ${
                view === "templates"
                  ? "bg-indigo-50 text-indigo-700"
                  : "text-slate-600 hover:text-slate-900"
              }`}
            >
              Templates
            </button>
            <button
              type="button"
              onClick={() => setView("builder")}
              className={`rounded-xl px-4 py-2 text-sm font-semibold transition ${
                view === "builder"
                  ? "bg-indigo-50 text-indigo-700"
                  : "text-slate-600 hover:text-slate-900"
              }`}
            >
              Builder
            </button>
          </div>
        </div>
      </header>

      {view === "templates" ? (
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
                    onChange={(event) => setTemplateQuery(event.target.value)}
                    placeholder="Search templates"
                    className="w-full rounded-xl border border-slate-200 bg-slate-50 px-10 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                </label>
                <select
                  value={templateCategory}
                  onChange={(event) => setTemplateCategory(event.target.value)}
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

            <div className="mt-6 grid gap-4 lg:grid-cols-2">
              {filteredTemplates.map((template) => {
                const isSelected = template.id === selectedTemplateId;
                return (
                  <article
                    key={template.id}
                    className={`rounded-3xl border p-5 shadow-sm transition ${
                      isSelected
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
                        <h3 className="mt-3 text-lg font-bold text-slate-900">
                          {template.name}
                        </h3>
                        <p className="mt-2 text-sm leading-6 text-slate-600">
                          {template.summary}
                        </p>
                      </div>
                      <div className="rounded-2xl bg-white px-3 py-2 text-right shadow-sm">
                        <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                          Tasks
                        </div>
                        <div className="mt-1 text-lg font-bold text-slate-900">
                          {template.tasks.length}
                        </div>
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
                      <div className="mt-1 font-semibold text-slate-800">
                        {template.recommendedRuntime}
                      </div>
                    </div>

                    <div className="mt-5 flex flex-wrap gap-3">
                      <button
                        type="button"
                        onClick={() => setSelectedTemplateId(template.id)}
                        className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-50"
                      >
                        Preview
                      </button>
                      <button
                        type="button"
                        onClick={() => applyTemplate(template.id)}
                        className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white hover:bg-slate-800"
                      >
                        Use template
                        <ArrowRight className="h-4 w-4" />
                      </button>
                    </div>
                  </article>
                );
              })}
            </div>
          </section>

          <aside className="h-fit rounded-3xl border border-slate-200 bg-white p-6 shadow-sm xl:sticky xl:top-8">
            {selectedTemplate ? (
              <>
                <div className="flex items-center gap-3">
                  <div className="rounded-2xl bg-indigo-50 p-3 text-indigo-600">
                    <Sparkles className="h-6 w-6" />
                  </div>
                  <div>
                    <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                      Template Preview
                    </div>
                    <h2 className="mt-1 text-xl font-bold text-slate-900">
                      {selectedTemplate.name}
                    </h2>
                  </div>
                </div>

                <p className="mt-4 text-sm leading-6 text-slate-600">
                  {selectedTemplate.description}
                </p>

                <div className="mt-5 rounded-2xl border border-slate-200 bg-slate-50 p-4">
                  <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                    Execution shape
                  </div>
                  <div className="mt-3 space-y-3">
                    {selectedTemplate.tasks.map((task, index) => (
                      <div
                        key={task.id}
                        className="rounded-2xl border border-slate-200 bg-white px-4 py-3"
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div>
                            <div className="text-sm font-semibold text-slate-900">
                              {task.id}
                            </div>
                            <div className="text-xs text-slate-500">{task.role}</div>
                          </div>
                          <div className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
                            {index + 1}
                          </div>
                        </div>
                        <div className="mt-2 text-xs leading-5 text-slate-600">
                          {task.prompt}
                        </div>
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

                {selectedTemplate.schedulerPreset && (
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
                  onClick={() => applyTemplate(selectedTemplate.id)}
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
        </div>
      ) : (
        <div className="grid gap-6 xl:grid-cols-[minmax(0,1.15fr)_360px]">
          <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
            <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
              <div>
                <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                  Workflow Builder
                </div>
                <h2 className="mt-2 text-2xl font-bold text-slate-900">
                  Compose the task graph
                </h2>
                <p className="mt-2 max-w-2xl text-sm text-slate-500">
                  Keep the current form-based workflow creation flow, then launch it
                  immediately or persist it as a scheduled job.
                </p>
              </div>
              <div className="w-full max-w-xs">
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

            {draftSourceTemplateId && (
              <div className="mt-5 rounded-2xl border border-indigo-200 bg-indigo-50 px-4 py-3 text-sm text-indigo-900">
                Builder initialized from template{" "}
                <strong>
                  {
                    workflowTemplates.find((template) => template.id === draftSourceTemplateId)
                      ?.name
                  }
                </strong>
                . You can now customize tasks and scheduler before launch.
              </div>
            )}

            <div className="mt-6">
              <WorkflowCanvasBuilder
                tasks={tasks}
                selectedTaskIndex={selectedTaskIndex}
                onSelectTask={setSelectedTaskIndex}
                onTasksChange={setTasks}
                onAddTask={addTask}
                onRemoveTask={removeTask}
              />
            </div>

            <section className="mt-6 rounded-3xl border border-slate-200 bg-slate-50 p-5">
              <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                <div>
                  <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                    Scheduler
                  </div>
                  <div className="mt-2 text-base font-semibold text-slate-900">
                    Optional durable trigger
                  </div>
                  <p className="mt-2 max-w-2xl text-sm text-slate-500">
                    Keep workflow creation and scheduling together, but move runtime
                    monitoring to Jobs.
                  </p>
                </div>
                <label className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs font-semibold uppercase tracking-wider text-slate-600">
                  <input
                    type="checkbox"
                    checked={schedulerDraft.enabled}
                    onChange={(event) =>
                      setSchedulerDraft((current) => ({
                        ...current,
                        enabled: event.target.checked,
                      }))
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        name: event.target.value,
                      }))
                    }
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        triggerKind: event.target.value as JobTriggerKind,
                      }))
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        timeoutSeconds: event.target.value,
                      }))
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        maxRetries: event.target.value,
                      }))
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        backoffSeconds: event.target.value,
                      }))
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        atLocal: event.target.value,
                      }))
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
                        setSchedulerDraft((current) => ({
                          ...current,
                          intervalSeconds: event.target.value,
                        }))
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
                        setSchedulerDraft((current) => ({
                          ...current,
                          startsAtLocal: event.target.value,
                        }))
                      }
                      className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
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
                      setSchedulerDraft((current) => ({
                        ...current,
                        cronExpression: event.target.value,
                      }))
                    }
                    placeholder="*/30 * * * *"
                    className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                  />
                </div>
              )}
            </section>

            {submitError && (
              <div className="mt-5 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
                {submitError}
              </div>
            )}
          </section>

          <aside className="space-y-6 xl:sticky xl:top-8 xl:h-fit">
            <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
              <div className="flex items-center gap-3">
                <div className="rounded-2xl bg-slate-100 p-3 text-slate-700">
                  <Layers3 className="h-6 w-6" />
                </div>
                <div>
                  <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                    Draft Overview
                  </div>
                  <h2 className="mt-1 text-xl font-bold text-slate-900">
                    {tasks.length} tasks ready
                  </h2>
                </div>
              </div>

              <div className="mt-5 grid gap-3 sm:grid-cols-2">
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Failure Policy
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {failurePolicy}
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Root Tasks
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {rootTasks.length}
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 sm:col-span-2">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Scheduler
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {formatSchedulerSummary(schedulerDraft)}
                  </div>
                </div>
              </div>

              <div className="mt-5">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  Validation
                </div>
                {validation.errors.length === 0 && validation.warnings.length === 0 ? (
                  <div className="mt-3 rounded-2xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700">
                    Visual graph and workflow payload are aligned.
                  </div>
                ) : (
                  <div className="mt-3 space-y-3">
                    {validation.errors.map((message) => (
                      <div
                        key={`error:${message}`}
                        className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700"
                      >
                        {message}
                      </div>
                    ))}
                    {validation.warnings.map((message) => (
                      <div
                        key={`warning:${message}`}
                        className="rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
                      >
                        {message}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </section>

            <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Task Inspector
              </div>
              <h2 className="mt-2 text-xl font-bold text-slate-900">
                {selectedTask && selectedTaskIndex !== null
                  ? taskDisplayId(selectedTask, selectedTaskIndex)
                  : "No task selected"}
              </h2>

              {selectedTask && selectedTaskIndex !== null ? (
                <div className="mt-5 space-y-4">
                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Task ID
                    </label>
                    <input
                      value={selectedTask.id}
                      onChange={(event) =>
                        updateTask(selectedTaskIndex, { id: event.target.value })
                      }
                      className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>

                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Role
                    </label>
                    <input
                      value={selectedTask.role}
                      onChange={(event) =>
                        updateTask(selectedTaskIndex, { role: event.target.value })
                      }
                      placeholder="Analyst, Reviewer, Synthesizer"
                      className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>

                  <div className="grid gap-4 sm:grid-cols-2">
                    <div>
                      <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                        Workload
                      </label>
                      <select
                        value={selectedTask.workload}
                        onChange={(event) =>
                          updateTask(selectedTaskIndex, {
                            workload: event.target.value as DraftWorkload,
                          })
                        }
                        className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
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
                        value={selectedTask.backendClass}
                        onChange={(event) =>
                          updateTask(selectedTaskIndex, {
                            backendClass: event.target.value as DraftBackendClass,
                          })
                        }
                        className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
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
                        value={selectedTask.contextStrategy}
                        onChange={(event) =>
                          updateTask(selectedTaskIndex, {
                            contextStrategy: event.target.value as DraftContextStrategy,
                          })
                        }
                        className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
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
                  </div>

                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Dependencies
                    </label>
                    <input
                      value={selectedTask.depsText}
                      onChange={(event) =>
                        updateTask(selectedTaskIndex, { depsText: event.target.value })
                      }
                      placeholder="Comma-separated task ids"
                      className="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>

                  <div>
                    <label className="mb-2 block text-xs font-semibold uppercase tracking-wider text-slate-500">
                      Prompt
                    </label>
                    <textarea
                      value={selectedTask.prompt}
                      onChange={(event) =>
                        updateTask(selectedTaskIndex, { prompt: event.target.value })
                      }
                      className="min-h-[180px] w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm leading-relaxed text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
                    />
                  </div>
                </div>
              ) : (
                <div className="mt-4 rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-sm text-slate-500">
                  Select a node from the visual builder to edit its details.
                </div>
              )}
            </section>

            <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Actions
              </div>
              <h2 className="mt-2 text-lg font-bold text-slate-900">
                Launch or schedule this graph
              </h2>

              <div className="mt-5 flex flex-col gap-3">
                <button
                  type="button"
                  onClick={handleLaunchWorkflow}
                  disabled={submittingMode !== null || validation.errors.length > 0}
                  className="inline-flex items-center justify-center gap-2 rounded-xl bg-slate-900 px-5 py-3 text-sm font-semibold text-white hover:bg-slate-800 disabled:opacity-40"
                >
                  <Waypoints className="h-4 w-4" />
                  {submittingMode === "launch" ? "Launching..." : "Launch workflow"}
                </button>
                <button
                  type="button"
                  onClick={handleScheduleWorkflow}
                  disabled={submittingMode !== null || validation.errors.length > 0}
                  className="inline-flex items-center justify-center gap-2 rounded-xl border border-indigo-200 bg-indigo-50 px-5 py-3 text-sm font-semibold text-indigo-700 hover:bg-indigo-100 disabled:opacity-40"
                >
                  <CalendarClock className="h-4 w-4" />
                  {submittingMode === "schedule" ? "Scheduling..." : "Schedule job"}
                </button>
                <button
                  type="button"
                  onClick={resetBuilder}
                  disabled={submittingMode !== null}
                  className="rounded-xl border border-slate-200 bg-white px-5 py-3 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-40"
                >
                  Reset draft
                </button>
              </div>

              {validation.errors.length > 0 && (
                <div className="mt-4 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900">
                  Resolve validation errors before launch or scheduling.
                </div>
              )}
            </section>

            <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
              <div className="flex items-center gap-3">
                <div className="rounded-2xl bg-amber-50 p-3 text-amber-700">
                  <CalendarClock className="h-6 w-6" />
                </div>
                <div>
                  <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                    Runtime separation
                  </div>
                  <h2 className="mt-1 text-lg font-bold text-slate-900">
                    Monitoring lives in Jobs
                  </h2>
                </div>
              </div>
              <p className="mt-4 text-sm leading-6 text-slate-600">
                Workflows is now the design surface. Live orchestrations, scheduled jobs
                and destructive controls stay in the dedicated runtime view.
              </p>
              <Link
                to="/jobs"
                className="mt-5 inline-flex items-center gap-2 rounded-xl bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 ring-1 ring-slate-200 hover:bg-slate-50"
              >
                Open Jobs
                <ArrowRight className="h-4 w-4" />
              </Link>
            </section>
          </aside>
        </div>
      )}
    </div>
  );
}
