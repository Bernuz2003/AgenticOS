import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Clock3,
  FileStack,
  Layers3,
  LoaderCircle,
  MessageSquareText,
  PauseCircle,
  RefreshCw,
  RotateCcw,
  Trash2,
  Waypoints,
} from "lucide-react";
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
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";

type InspectorTab = "details" | "transcript" | "artifacts" | "events" | "messages";

function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
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

function formatReasonLabel(reason: string | null | undefined): string {
  if (!reason) {
    return "n/a";
  }
  return reason.split("_").join(" ");
}

function taskTone(status: string): string {
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

function progressPercent(detail: OrchestrationStatus): number {
  if (detail.total <= 0) {
    return 0;
  }
  return Math.round(
    ((detail.completed + detail.failed + detail.skipped) / detail.total) * 100,
  );
}

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
      <header className="rounded-[32px] border border-slate-200 bg-white px-8 py-8 shadow-sm">
        <div className="flex flex-col gap-6 xl:flex-row xl:items-end xl:justify-between">
          <div className="max-w-3xl">
            <div className="text-xs font-bold uppercase tracking-[0.28em] text-slate-400">
              Workflow Run Detail
            </div>
            <h1 className="mt-3 text-3xl font-bold tracking-tight text-slate-900">
              Run #{detail.orchestrationId}
            </h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              Transcript, artifacts, attempts and runtime events stay inside the
              workflow context instead of leaking into global chats.
            </p>
          </div>
          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              onClick={() => void reloadDetail()}
              className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
            <Link
              to="/jobs"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Back to Jobs
            </Link>
            <button
              type="button"
              onClick={() => void handleStop()}
              disabled={detail.finished || busyKey === "stop"}
              className="inline-flex items-center gap-2 rounded-xl border border-amber-200 bg-amber-50 px-4 py-2.5 text-sm font-semibold text-amber-700 hover:bg-amber-100 disabled:opacity-40"
            >
              <PauseCircle className="h-4 w-4" />
              {busyKey === "stop" ? "Stopping..." : "Stop"}
            </button>
            <button
              type="button"
              onClick={() => void handleDelete()}
              disabled={!detail.finished || busyKey === "delete"}
              className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-4 py-2.5 text-sm font-semibold text-rose-700 hover:bg-rose-100 disabled:opacity-40"
            >
              <Trash2 className="h-4 w-4" />
              {busyKey === "delete" ? "Deleting..." : "Delete"}
            </button>
          </div>
        </div>

        <div className="mt-8 h-2 overflow-hidden rounded-full bg-slate-200">
          <div
            className="h-full rounded-full bg-gradient-to-r from-indigo-500 to-sky-400"
            style={{ width: `${progress}%` }}
          />
        </div>

        <div className="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-6">
          {[
            ["Total", detail.total, "text-slate-900"],
            ["Completed", detail.completed, "text-sky-700"],
            ["Running", detail.running, "text-emerald-700"],
            ["Pending", detail.pending, "text-amber-700"],
            ["Failed", detail.failed, "text-rose-700"],
            ["Skipped", detail.skipped, "text-amber-700"],
          ].map(([label, value, tone]) => (
            <div
              key={label}
              className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4"
            >
              <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                {label}
              </div>
              <div className={`mt-1 text-2xl font-bold ${tone}`}>{value}</div>
            </div>
          ))}
        </div>

        <div className="mt-6 flex flex-wrap gap-2 text-[11px] text-slate-500">
          <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 font-semibold text-indigo-700">
            {detail.policy}
          </span>
          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
            elapsed {formatElapsed(detail.elapsedSecs)}
          </span>
          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
            stored chars {detail.outputCharsStored.toLocaleString()}
          </span>
          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
            truncations {detail.truncations}
          </span>
          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
            ipc messages {detail.ipcMessages.length}
          </span>
          {runTerminationReasons.map((reason) => (
            <span
              key={reason}
              className="rounded-full border border-slate-200 bg-white px-2.5 py-1"
            >
              {formatReasonLabel(reason)}
            </span>
          ))}
        </div>
      </header>

      {error && (
        <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {error}
        </div>
      )}

      <section className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_380px]">
        <div className="space-y-6">
          <div className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
            <div className="flex items-center gap-3">
              <div className="rounded-2xl bg-slate-100 p-3 text-slate-700">
                <Waypoints className="h-6 w-6" />
              </div>
              <div>
                <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                  Execution Map
                </div>
                <h2 className="mt-1 text-xl font-bold text-slate-900">
                  Task graph
                </h2>
              </div>
            </div>

            <div className="mt-5 grid gap-4 lg:grid-cols-2">
              {detail.tasks.map((task) => (
                <button
                  key={task.task}
                  type="button"
                  onClick={() => setSelectedTaskId(task.task)}
                  className={`rounded-3xl border p-5 text-left shadow-sm transition ${
                    selectedTask?.task === task.task
                      ? "border-indigo-200 bg-indigo-50/70"
                      : "border-slate-200 bg-slate-50 hover:border-slate-300"
                  }`}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className="text-base font-semibold text-slate-900">{task.task}</div>
                      <div className="mt-1 text-sm text-slate-500">
                        {task.role ?? "Unassigned role"}
                      </div>
                    </div>
                    <span
                      className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskTone(
                        task.status,
                      )}`}
                    >
                      {task.status}
                    </span>
                  </div>

                  <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
                    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                      workload {task.workload ?? "default"}
                    </span>
                    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                      context {task.contextStrategy ?? "kernel_default"}
                    </span>
                    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                      deps {task.deps.length === 0 ? "root" : task.deps.join(", ")}
                    </span>
                  </div>

                  {task.latestOutputPreview && (
                    <div className="mt-4 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-xs leading-6 text-slate-600">
                      {task.latestOutputPreview}
                    </div>
                  )}

                  <div className="mt-4 flex flex-wrap gap-2 text-[11px] text-slate-500">
                    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                      attempts {task.attempts.length}
                    </span>
                    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                      artifacts {task.outputArtifacts.length}
                    </span>
                    {task.terminationReason && (
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        {formatReasonLabel(task.terminationReason)}
                      </span>
                    )}
                    {task.currentAttempt !== null && (
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        current {task.currentAttempt}
                      </span>
                    )}
                  </div>
                </button>
              ))}
            </div>
          </div>
        </div>

        <aside className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm xl:sticky xl:top-8 xl:h-fit">
          <div className="flex items-start gap-3">
            <div className="rounded-2xl bg-indigo-50 p-3 text-indigo-600">
              <Layers3 className="h-6 w-6" />
            </div>
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Inspector
              </div>
              <h2 className="mt-1 text-xl font-bold text-slate-900">
                {selectedTask?.task ?? "No task selected"}
              </h2>
              <div className="mt-2 flex flex-wrap gap-2">
                {selectedTask && (
                  <span
                    className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskTone(
                      selectedTask.status,
                    )}`}
                  >
                    {selectedTask.status}
                  </span>
                )}
                {selectedTask?.role && (
                  <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                    {selectedTask.role}
                  </span>
                )}
              </div>
            </div>
          </div>

          <div className="mt-5 grid grid-cols-5 gap-2">
            {[
              { id: "details" as InspectorTab, label: "Details", icon: FileStack },
              {
                id: "transcript" as InspectorTab,
                label: "Transcript",
                icon: MessageSquareText,
              },
              { id: "artifacts" as InspectorTab, label: "Artifacts", icon: Layers3 },
              { id: "events" as InspectorTab, label: "Events", icon: Clock3 },
              { id: "messages" as InspectorTab, label: "IPC", icon: Waypoints },
            ].map((entry) => {
              const Icon = entry.icon;
              return (
              <button
                key={entry.id}
                type="button"
                onClick={() => setInspectorTab(entry.id)}
                className={`rounded-2xl border px-3 py-3 text-xs font-semibold transition ${
                  inspectorTab === entry.id
                    ? "border-indigo-200 bg-indigo-50 text-indigo-700"
                    : "border-slate-200 bg-slate-50 text-slate-600 hover:bg-slate-100"
                }`}
              >
                <Icon className="mx-auto mb-2 h-4 w-4" />
                {entry.label}
              </button>
              );
            })}
          </div>

          {selectedTask && inspectorTab === "details" && (
            <div className="mt-6 space-y-4">
              {selectedWorkspace?.pendingHumanRequest && (
                <div className="rounded-2xl border border-sky-200 bg-sky-50 p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-sky-600">
                        Human Input Pending
                      </div>
                      <div className="mt-2 text-sm font-semibold text-slate-900">
                        {selectedWorkspace.pendingHumanRequest.question}
                      </div>
                    </div>
                    <span className="rounded-full border border-sky-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-sky-700">
                      {selectedWorkspace.pendingHumanRequest.kind}
                    </span>
                  </div>

                  {selectedWorkspace.pendingHumanRequest.details && (
                    <div className="mt-3 rounded-2xl border border-sky-100 bg-white px-4 py-3 text-sm leading-6 text-slate-700">
                      {selectedWorkspace.pendingHumanRequest.details}
                    </div>
                  )}

                  {selectedWorkspace.pendingHumanRequest.choices.length > 0 && (
                    <div className="mt-4 flex flex-wrap gap-2">
                      {selectedWorkspace.pendingHumanRequest.choices.map((choice) => (
                        <button
                          key={choice}
                          type="button"
                          onClick={() => void handleHumanReply(choice)}
                          disabled={humanReplyBusy}
                          className="rounded-xl border border-sky-200 bg-white px-4 py-2 text-sm font-semibold text-sky-800 transition hover:border-sky-300 hover:bg-sky-100 disabled:opacity-50"
                        >
                          {choice}
                        </button>
                      ))}
                    </div>
                  )}

                  {selectedWorkspace.pendingHumanRequest.allowFreeText && (
                    <div className="mt-4 space-y-3">
                      <textarea
                        value={humanReply}
                        onChange={(event) => setHumanReply(event.target.value)}
                        rows={4}
                        placeholder={
                          selectedWorkspace.pendingHumanRequest.placeholder ??
                          "Provide the human response needed to resume this workflow task..."
                        }
                        className="w-full rounded-2xl border border-sky-200 bg-white px-4 py-3 text-sm text-slate-800 outline-none transition focus:border-sky-400 focus:ring-2 focus:ring-sky-100"
                      />
                      <div className="flex items-center justify-between gap-3">
                        <div className="text-xs text-slate-500">
                          request {selectedWorkspace.pendingHumanRequest.requestId}
                        </div>
                        <button
                          type="button"
                          onClick={() => void handleHumanReply(humanReply)}
                          disabled={humanReplyBusy || !humanReply.trim()}
                          className="rounded-xl border border-sky-200 bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700 disabled:opacity-50"
                        >
                          {humanReplyBusy ? "Sending..." : "Resume task"}
                        </button>
                      </div>
                    </div>
                  )}

                  {humanReplyError && (
                    <div className="mt-3 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
                      {humanReplyError}
                    </div>
                  )}
                </div>
              )}

              <div className="grid gap-3 sm:grid-cols-2">
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Workload
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {selectedTask.workload ?? "default"}
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Backend
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {selectedTask.backendClass ?? "auto"}
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Context
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {selectedTask.contextStrategy ?? "kernel_default"}
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                  <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
                    Dependencies
                  </div>
                  <div className="mt-1 text-sm font-semibold text-slate-900">
                    {selectedTask.deps.length === 0 ? "root" : selectedTask.deps.join(", ")}
                  </div>
                </div>
              </div>

              {selectedTask.error && (
                <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
                  {selectedTask.error}
                </div>
              )}

              {selectedTask.terminationReason && (
                <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-700">
                  Termination reason:{" "}
                  <span className="font-semibold">
                    {formatReasonLabel(selectedTask.terminationReason)}
                  </span>
                </div>
              )}

              <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  {selectedTask.status === "running" ? "Live Output" : "Latest Result Artifact"}
                </div>
                <div className="mt-3 whitespace-pre-wrap break-words text-sm leading-6 text-slate-700">
                  {selectedTask.latestOutputText ??
                    selectedTask.latestOutputPreview ??
                    "No output captured yet."}
                </div>
              </div>

              <div className="flex flex-wrap gap-3">
                <button
                  type="button"
                  onClick={() => void handleRetryTask(selectedTask.task)}
                  disabled={!canRetrySelectedTask || busyKey === `retry:${selectedTask.task}`}
                  className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50 disabled:opacity-40"
                >
                  <RotateCcw className="h-4 w-4" />
                  {busyKey === `retry:${selectedTask.task}` ? "Retrying..." : "Retry task"}
                </button>
              </div>
            </div>
          )}

          {selectedTask && inspectorTab === "artifacts" && (
            <div className="mt-6 space-y-4">
              <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  Input Artifacts
                </div>
                {selectedTask.inputArtifacts.length === 0 ? (
                  <div className="mt-3 text-sm text-slate-500">No upstream artifacts.</div>
                ) : (
                  <div className="mt-3 flex flex-wrap gap-2">
                    {selectedTask.inputArtifacts.map((artifact) => (
                      <span
                        key={artifact.artifactId}
                        className="rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1 text-[11px] font-semibold text-emerald-700"
                      >
                        {artifact.task} #{artifact.attempt} · {artifact.label}
                      </span>
                    ))}
                  </div>
                )}
              </div>

              <div className="space-y-3">
                {selectedTask.outputArtifacts.length === 0 ? (
                  <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
                    No persisted artifacts yet.
                  </div>
                ) : (
                  selectedTask.outputArtifacts.map((artifact) => (
                    <article
                      key={artifact.artifactId}
                      className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
                    >
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="text-sm font-semibold text-slate-900">
                            {artifact.label}
                          </div>
                          <div className="mt-1 text-xs text-slate-500">
                            {artifact.kind} · attempt {artifact.attempt}
                          </div>
                        </div>
                        <div className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[11px] font-semibold text-slate-600">
                          {formatBytes(artifact.bytes)}
                        </div>
                      </div>
                      <div className="mt-3 whitespace-pre-wrap break-words text-sm leading-6 text-slate-700">
                        {artifact.content || artifact.preview || "Empty artifact"}
                      </div>
                    </article>
                  ))
                )}
              </div>
            </div>
          )}

          {selectedTask && inspectorTab === "events" && (
            <div className="mt-6 space-y-4">
              <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  Attempt History
                </div>
                <div className="mt-3 space-y-3">
                  {selectedTask.attempts.map((attempt) => (
                    <div
                      key={`${selectedTask.task}-${attempt.attempt}`}
                      className="rounded-2xl border border-slate-200 bg-white px-4 py-3"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                            attempt {attempt.attempt}
                          </span>
                          <span
                            className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskTone(
                              attempt.status,
                            )}`}
                          >
                            {attempt.status}
                          </span>
                        </div>
                        <div className="text-[11px] text-slate-500">
                          {formatTimestamp(attempt.startedAtMs)}
                        </div>
                      </div>
                      <div className="mt-2 flex flex-wrap gap-2 text-[11px] text-slate-500">
                        <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                          chars {attempt.outputChars.toLocaleString()}
                        </span>
                        {attempt.completedAtMs && (
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            completed {formatTimestamp(attempt.completedAtMs)}
                          </span>
                        )}
                        {attempt.terminationReason && (
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            reason {formatReasonLabel(attempt.terminationReason)}
                          </span>
                        )}
                        {attempt.pid && (
                          <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1">
                            pid {attempt.pid}
                          </span>
                        )}
                      </div>
                      {attempt.error && (
                        <div className="mt-3 text-xs text-rose-700">{attempt.error}</div>
                      )}
                    </div>
                  ))}
                </div>
              </div>

              <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  Runtime Events
                </div>
                {workspaceError && (
                  <div className="mt-3 text-sm text-rose-700">{workspaceError}</div>
                )}
                {selectedWorkspace?.auditEvents.length ? (
                  <div className="mt-3 space-y-2">
                    {selectedWorkspace.auditEvents
                      .slice()
                      .sort((left, right) => right.recordedAtMs - left.recordedAtMs)
                      .slice(0, 12)
                      .map((event) => (
                        <div
                          key={`${event.recordedAtMs}:${event.category}:${event.kind}:${event.detail}`}
                          className="rounded-2xl border border-slate-200 bg-white px-4 py-3"
                        >
                          <div className="flex items-center justify-between gap-3">
                            <div className="text-sm font-semibold text-slate-900">
                              {event.title}
                            </div>
                            <div className="text-[11px] text-slate-500">
                              {formatTimestamp(event.recordedAtMs)}
                            </div>
                          </div>
                          <div className="mt-1 text-xs uppercase tracking-wider text-slate-400">
                            {event.category} · {event.kind}
                          </div>
                          <div className="mt-2 text-sm leading-6 text-slate-600">
                            {event.detail}
                          </div>
                        </div>
                      ))}
                  </div>
                ) : (
                  <div className="mt-3 text-sm text-slate-500">
                    No runtime audit events available for the selected attempt.
                  </div>
                )}
              </div>
            </div>
          )}

          {selectedTask && inspectorTab === "messages" && (
            <div className="mt-6 space-y-4">
              <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                  IPC / Message Bus
                </div>
                <div className="mt-2 text-sm text-slate-500">
                  Structured process-to-process messages for this task and run.
                </div>
              </div>

              {selectedMessages.length === 0 ? (
                <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
                  No IPC messages captured for the selected task yet.
                </div>
              ) : (
                selectedMessages.map((message) => (
                  <article
                    key={message.messageId}
                    className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-indigo-700">
                          {message.messageType}
                        </span>
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                          {message.status}
                        </span>
                        {message.channel && (
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                            {message.channel}
                          </span>
                        )}
                      </div>
                      <div className="text-[11px] text-slate-500">
                        {formatTimestamp(message.createdAtMs)}
                      </div>
                    </div>

                    <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        from {message.senderTask ?? `pid ${message.senderPid ?? "?"}`}
                      </span>
                      <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                        to{" "}
                        {message.receiverTask ??
                          (message.receiverRole
                            ? `role ${message.receiverRole}`
                            : `pid ${message.receiverPid ?? "?"}`)}
                      </span>
                      {message.senderAttempt !== null && (
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                          sender attempt {message.senderAttempt}
                        </span>
                      )}
                      {message.receiverAttempt !== null && (
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                          receiver attempt {message.receiverAttempt}
                        </span>
                      )}
                      {message.deliveredAtMs !== null && (
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                          delivered {formatTimestamp(message.deliveredAtMs)}
                        </span>
                      )}
                      {message.consumedAtMs !== null && (
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                          consumed {formatTimestamp(message.consumedAtMs)}
                        </span>
                      )}
                      {message.failedAtMs !== null && (
                        <span className="rounded-full border border-rose-200 bg-rose-50 px-2.5 py-1 text-rose-700">
                          failed {formatTimestamp(message.failedAtMs)}
                        </span>
                      )}
                    </div>

                    <div className="mt-3 whitespace-pre-wrap break-words rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm leading-6 text-slate-700">
                      {message.payloadText || message.payloadPreview}
                    </div>
                  </article>
                ))
              )}
            </div>
          )}

          {selectedTask && inspectorTab === "transcript" && (
            <div className="mt-6 space-y-4">
              {timelineError && (
                <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
                  {timelineError}
                </div>
              )}

              {timelineLoading ? (
                <div className="flex items-center gap-2 rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-600">
                  <LoaderCircle className="h-4 w-4 animate-spin text-indigo-500" />
                  Loading transcript...
                </div>
              ) : selectedTimeline?.items.length ? (
                <div className="space-y-3">
                  {selectedTimeline.items.map((item) => (
                    <article
                      key={item.id}
                      className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
                    >
                      <div className="mb-3 flex items-center justify-between gap-3">
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                          {item.kind}
                        </span>
                        <span className="text-[11px] uppercase tracking-wider text-slate-400">
                          {item.status}
                        </span>
                      </div>
                      {item.kind === "assistant_message" ? (
                        <div className="prose prose-slate max-w-none text-sm prose-p:leading-relaxed prose-pre:bg-slate-900 prose-pre:text-slate-50">
                          <Markdown remarkPlugins={[remarkGfm]}>{item.text}</Markdown>
                        </div>
                      ) : (
                        <div className="whitespace-pre-wrap break-words text-sm leading-6 text-slate-700">
                          {item.text}
                        </div>
                      )}
                    </article>
                  ))}
                </div>
              ) : (
                <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
                  No transcript available for the selected task attempt.
                </div>
              )}
            </div>
          )}
        </aside>
      </section>
    </div>
  );
}
