import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Clock3,
  FileStack,
  Layers3,
  LoaderCircle,
  MessageSquareText,
  RotateCcw,
  Waypoints,
} from "lucide-react";

import type { OrchestrationStatus, TimelineSnapshot, WorkspaceSnapshot } from "../../lib/api";
import { WorkflowRunStatusBadge } from "../../components/workflows/run/status-badge";
import { WorkflowArtifactsPanel } from "./artifacts-panel";
import { WorkflowEventsPanel } from "./events-panel";
import { WorkflowIpcPanel } from "./ipc-panel";
import { formatReasonLabel } from "./format";
import type { InspectorTab } from "./format";

type WorkflowTask = OrchestrationStatus["tasks"][number];

interface WorkflowInspectorProps {
  selectedTask: WorkflowTask | null;
  inspectorTab: InspectorTab;
  onInspectorTabChange: (tab: InspectorTab) => void;
  selectedTimeline: TimelineSnapshot | null;
  timelineLoading: boolean;
  timelineError: string | null;
  selectedWorkspace: WorkspaceSnapshot | null;
  workspaceError: string | null;
  selectedMessages: OrchestrationStatus["ipcMessages"];
  canRetrySelectedTask: boolean;
  busyKey: string | null;
  humanReply: string;
  humanReplyBusy: boolean;
  humanReplyError: string | null;
  onHumanReplyChange: (value: string) => void;
  onHumanReply: (reply: string) => void | Promise<void>;
  onRetryTask: (taskId: string) => void | Promise<void>;
}

const INSPECTOR_TABS: {
  id: InspectorTab;
  label: string;
  icon: typeof FileStack;
}[] = [
  { id: "details", label: "Details", icon: FileStack },
  { id: "transcript", label: "Transcript", icon: MessageSquareText },
  { id: "artifacts", label: "Artifacts", icon: Layers3 },
  { id: "events", label: "Events", icon: Clock3 },
  { id: "messages", label: "IPC", icon: Waypoints },
];

export function WorkflowInspector({
  selectedTask,
  inspectorTab,
  onInspectorTabChange,
  selectedTimeline,
  timelineLoading,
  timelineError,
  selectedWorkspace,
  workspaceError,
  selectedMessages,
  canRetrySelectedTask,
  busyKey,
  humanReply,
  humanReplyBusy,
  humanReplyError,
  onHumanReplyChange,
  onHumanReply,
  onRetryTask,
}: WorkflowInspectorProps) {
  return (
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
            {selectedTask && <WorkflowRunStatusBadge status={selectedTask.status} />}
            {selectedTask?.role && (
              <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                {selectedTask.role}
              </span>
            )}
          </div>
        </div>
      </div>

      <div className="mt-5 grid grid-cols-5 gap-2">
        {INSPECTOR_TABS.map((entry) => {
          const Icon = entry.icon;
          return (
            <button
              key={entry.id}
              type="button"
              onClick={() => onInspectorTabChange(entry.id)}
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
                      onClick={() => void onHumanReply(choice)}
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
                    onChange={(event) => onHumanReplyChange(event.target.value)}
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
                      onClick={() => void onHumanReply(humanReply)}
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
              onClick={() => void onRetryTask(selectedTask.task)}
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
        <WorkflowArtifactsPanel task={selectedTask} />
      )}

      {selectedTask && inspectorTab === "events" && (
        <WorkflowEventsPanel
          task={selectedTask}
          workspace={selectedWorkspace}
          workspaceError={workspaceError}
        />
      )}

      {selectedTask && inspectorTab === "messages" && (
        <WorkflowIpcPanel messages={selectedMessages} />
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
  );
}
