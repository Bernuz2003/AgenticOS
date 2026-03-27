import { useEffect, useMemo, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { useNavigate, useParams } from "react-router-dom";

import {
  deleteWorkflowRun,
  fetchOrchestrationStatus,
  fetchTimelineSnapshot,
  fetchWorkspaceSnapshot,
  retryWorkflowTask,
  sendSessionInput,
  stopWorkflowRun,
  type OrchestrationStatus,
  type TimelineSnapshot,
  type WorkspaceSnapshot,
} from "../../lib/api";
import { useSessionsStore } from "../../store/sessions-store";
import { WorkflowRunHeader } from "./header";
import { WorkflowInspector } from "./inspector";
import { WorkflowTaskGraph } from "./task-graph";
import { type InspectorTab, progressPercent } from "./format";

export function WorkflowRunPage() {
  const navigate = useNavigate();
  const { orchestrationId: orchestrationIdParam } = useParams();
  const orchestrationId = Number.parseInt(orchestrationIdParam ?? "", 10);
  const refreshLobby = useSessionsStore((state) => state.refresh);
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const [detail, setDetail] = useState<OrchestrationStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("details");
  const [selectedTimeline, setSelectedTimeline] = useState<TimelineSnapshot | null>(null);
  const [timelineLoading, setTimelineLoading] = useState(false);
  const [timelineError, setTimelineError] = useState<string | null>(null);
  const [selectedWorkspace, setSelectedWorkspace] = useState<WorkspaceSnapshot | null>(null);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  const [humanReply, setHumanReply] = useState("");
  const [humanReplyBusy, setHumanReplyBusy] = useState(false);
  const [humanReplyError, setHumanReplyError] = useState<string | null>(null);

  const orchestrationSignature = useMemo(
    () =>
      orchestrations
        .map(
          (workflow) =>
            `${workflow.orchestrationId}:${workflow.running}:${workflow.pending}:${workflow.completed}:${workflow.failed}:${workflow.skipped}:${workflow.finished}`,
        )
        .join("|"),
    [orchestrations],
  );

  useEffect(() => {
    if (!Number.isFinite(orchestrationId)) {
      setLoading(false);
      setError("Invalid orchestration id");
      return;
    }

    let cancelled = false;
    const load = async () => {
      setLoading(true);
      setError(null);
      try {
        const nextDetail = await fetchOrchestrationStatus(orchestrationId);
        if (cancelled) {
          return;
        }
        setDetail(nextDetail);
      } catch (loadError) {
        if (cancelled) {
          return;
        }
        setError(
          loadError instanceof Error
            ? loadError.message
            : "Failed to fetch workflow run status",
        );
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [orchestrationId, orchestrationSignature]);

  useEffect(() => {
    if (!detail || detail.tasks.length === 0) {
      setSelectedTaskId(null);
      return;
    }
    if (selectedTaskId && detail.tasks.some((task) => task.task === selectedTaskId)) {
      return;
    }
    const preferredTask =
      detail.tasks.find((task) => task.status === "running")?.task ??
      detail.tasks.find((task) => task.status === "failed")?.task ??
      detail.tasks[0]?.task ??
      null;
    setSelectedTaskId(preferredTask);
  }, [detail, selectedTaskId]);

  const selectedTask =
    detail?.tasks.find((task) => task.task === selectedTaskId) ?? detail?.tasks[0] ?? null;
  const selectedAttempt = useMemo(() => {
    if (!selectedTask) {
      return null;
    }
    return (
      selectedTask.attempts.find(
        (attempt) =>
          attempt.attempt === selectedTask.currentAttempt && attempt.sessionId,
      ) ??
      [...selectedTask.attempts]
        .filter((attempt) => attempt.sessionId)
        .sort((left, right) => right.attempt - left.attempt)[0] ??
      null
    );
  }, [selectedTask]);
  const selectedMessages = useMemo(() => {
    if (!detail) {
      return [];
    }
    if (!selectedTask) {
      return detail.ipcMessages;
    }
    return detail.ipcMessages.filter(
      (message) =>
        message.senderTask === selectedTask.task ||
        message.receiverTask === selectedTask.task ||
        (selectedTask.role !== null && message.receiverRole === selectedTask.role),
    );
  }, [detail, selectedTask]);
  const runTerminationReasons = useMemo(
    () =>
      detail
        ? [
            ...new Set(
              detail.tasks
                .flatMap((task) => task.attempts.map((attempt) => attempt.terminationReason))
                .filter((value): value is string => Boolean(value)),
            ),
          ]
        : [],
    [detail],
  );

  useEffect(() => {
    setHumanReply("");
    setHumanReplyError(null);
    setHumanReplyBusy(false);
  }, [selectedAttempt?.pid, selectedWorkspace?.pendingHumanRequest?.requestId]);

  useEffect(() => {
    if (!selectedAttempt?.sessionId) {
      setSelectedTimeline(null);
      setSelectedWorkspace(null);
      setTimelineError(null);
      setWorkspaceError(null);
      return;
    }

    let cancelled = false;
    const load = async () => {
      const sessionId = selectedAttempt.sessionId;
      if (!sessionId) {
        return;
      }
      setTimelineLoading(true);
      setTimelineError(null);
      setWorkspaceError(null);
      try {
        const [timeline, workspace] = await Promise.all([
          fetchTimelineSnapshot(sessionId, selectedAttempt.pid),
          fetchWorkspaceSnapshot(sessionId, selectedAttempt.pid),
        ]);
        if (cancelled) {
          return;
        }
        setSelectedTimeline(timeline);
        setSelectedWorkspace(workspace);
      } catch (loadError) {
        if (cancelled) {
          return;
        }
        const message =
          loadError instanceof Error
            ? loadError.message
            : "Failed to fetch task transcript";
        setTimelineError(message);
        setWorkspaceError(message);
      } finally {
        if (!cancelled) {
          setTimelineLoading(false);
        }
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [selectedAttempt?.pid, selectedAttempt?.sessionId]);

  async function reloadDetail() {
    if (!Number.isFinite(orchestrationId)) {
      return;
    }
    const nextDetail = await fetchOrchestrationStatus(orchestrationId);
    setDetail(nextDetail);
  }

  async function handleStop() {
    if (!detail) {
      return;
    }
    setBusyKey("stop");
    setError(null);
    try {
      await stopWorkflowRun(detail.orchestrationId);
      await refreshLobby();
      await reloadDetail();
    } catch (stopError) {
      setError(
        stopError instanceof Error ? stopError.message : "Failed to stop workflow run",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function handleDelete() {
    if (!detail) {
      return;
    }
    setBusyKey("delete");
    setError(null);
    try {
      await deleteWorkflowRun(detail.orchestrationId);
      await refreshLobby();
      navigate("/jobs");
    } catch (deleteError) {
      setError(
        deleteError instanceof Error
          ? deleteError.message
          : "Failed to delete workflow run",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function handleRetryTask(taskId: string) {
    if (!detail) {
      return;
    }
    setBusyKey(`retry:${taskId}`);
    setError(null);
    try {
      await retryWorkflowTask(detail.orchestrationId, taskId);
      await refreshLobby();
      await reloadDetail();
    } catch (retryError) {
      setError(
        retryError instanceof Error ? retryError.message : "Failed to retry workflow task",
      );
    } finally {
      setBusyKey(null);
    }
  }

  async function refreshSelectedAttempt() {
    if (!selectedAttempt?.sessionId || !selectedAttempt.pid) {
      return;
    }
    const [timeline, workspace] = await Promise.all([
      fetchTimelineSnapshot(selectedAttempt.sessionId, selectedAttempt.pid),
      fetchWorkspaceSnapshot(selectedAttempt.sessionId, selectedAttempt.pid),
    ]);
    setSelectedTimeline(timeline);
    setSelectedWorkspace(workspace);
  }

  async function handleHumanReply(rawReply: string) {
    if (!selectedAttempt?.pid) {
      return;
    }
    const reply = rawReply.trim();
    if (!reply) {
      return;
    }

    setHumanReplyBusy(true);
    setHumanReplyError(null);
    setWorkspaceError(null);
    try {
      await sendSessionInput({
        pid: selectedAttempt.pid,
        sessionId: selectedAttempt.sessionId,
        prompt: reply,
      });
      setHumanReply("");
      await Promise.all([refreshLobby(), reloadDetail(), refreshSelectedAttempt()]);
    } catch (replyError) {
      setHumanReplyError(
        replyError instanceof Error
          ? replyError.message
          : "Failed to send human response to workflow task",
      );
    } finally {
      setHumanReplyBusy(false);
    }
  }

  if (!Number.isFinite(orchestrationId)) {
    return (
      <div className="rounded-3xl border border-rose-200 bg-rose-50 px-6 py-8 text-rose-800">
        Invalid workflow run identifier.
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex min-h-[40vh] items-center justify-center">
        <div className="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white px-4 py-2 text-sm text-slate-600 shadow-sm">
          <LoaderCircle className="h-4 w-4 animate-spin text-indigo-500" />
          Loading workflow run...
        </div>
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="rounded-3xl border border-rose-200 bg-rose-50 px-6 py-8 text-rose-800">
        {error ?? "Workflow run not found."}
      </div>
    );
  }

  const progress = progressPercent(detail);
  const canRetrySelectedTask =
    !!selectedTask &&
    selectedTask.status !== "running" &&
    (selectedTask.attempts.length > 0 || selectedTask.status === "skipped");

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <WorkflowRunHeader
        detail={detail}
        busyKey={busyKey}
        progress={progress}
        runTerminationReasons={runTerminationReasons}
        onReload={reloadDetail}
        onStop={handleStop}
        onDelete={handleDelete}
      />

      {error && (
        <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {error}
        </div>
      )}

      <section className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_380px]">
        <div className="space-y-6">
          <WorkflowTaskGraph
            detail={detail}
            selectedTaskId={selectedTask?.task ?? null}
            onSelectTask={setSelectedTaskId}
          />
        </div>

        <WorkflowInspector
          selectedTask={selectedTask}
          inspectorTab={inspectorTab}
          onInspectorTabChange={setInspectorTab}
          selectedTimeline={selectedTimeline}
          timelineLoading={timelineLoading}
          timelineError={timelineError}
          selectedWorkspace={selectedWorkspace}
          workspaceError={workspaceError}
          selectedMessages={selectedMessages}
          canRetrySelectedTask={canRetrySelectedTask}
          busyKey={busyKey}
          humanReply={humanReply}
          humanReplyBusy={humanReplyBusy}
          humanReplyError={humanReplyError}
          onHumanReplyChange={setHumanReply}
          onHumanReply={handleHumanReply}
          onRetryTask={handleRetryTask}
        />
      </section>
    </div>
  );
}
