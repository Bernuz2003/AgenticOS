import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { orchestrate, scheduleWorkflowJob } from "../../../lib/api";
import { useSessionsStore } from "../../../store/sessions-store";
import {
  buildSchedulePayload,
  buildWorkflowPayload,
  createTask,
  initialSchedulerDraft,
  initialTasks,
  type DraftTask,
  type FailurePolicy,
  type SchedulerDraft,
} from "../../../lib/workflow-builder";
import { validateWorkflowDraft } from "../../../lib/workflow-builder/graph";
import {
  instantiateTemplateDraft,
  workflowTemplateCategories,
  workflowTemplates,
} from "../../../lib/workflow-templates";

export function useWorkflowBuilder() {
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
  const [submittingMode, setSubmittingMode] = useState<"launch" | "schedule" | null>(
    null,
  );

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
  const selectedTask =
    (selectedTaskIndex !== null ? tasks[selectedTaskIndex] : null) ?? tasks[0] ?? null;
  const rootTasks = tasks.filter((task) => task.depsText.trim() === "");
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

  const draftSourceTemplateName =
    draftSourceTemplateId === null
      ? null
      : workflowTemplates.find((template) => template.id === draftSourceTemplateId)?.name ??
        null;

  return {
    view,
    setView,
    templateQuery,
    setTemplateQuery,
    templateCategory,
    setTemplateCategory,
    selectedTemplateId,
    setSelectedTemplateId,
    selectedTemplate,
    categories,
    filteredTemplates,
    draftSourceTemplateName,
    failurePolicy,
    setFailurePolicy,
    tasks,
    setTasks,
    selectedTaskIndex,
    setSelectedTaskIndex,
    selectedTask,
    schedulerDraft,
    setSchedulerDraft,
    rootTasks,
    validation,
    submitError,
    submittingMode,
    updateTask,
    addTask,
    removeTask,
    resetBuilder,
    applyTemplate,
    handleLaunchWorkflow,
    handleScheduleWorkflow,
  };
}

export type WorkflowBuilderViewModel = ReturnType<typeof useWorkflowBuilder>;
