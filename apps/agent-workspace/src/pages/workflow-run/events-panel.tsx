import type { OrchestrationStatus, WorkspaceSnapshot } from "../../lib/api";
import { PreviewRecordList } from "../../components/ui/preview-record-list";
import { WorkflowRunStatusBadge } from "../../components/workflows/run/status-badge";
import { formatReasonLabel, formatTimestamp } from "./format";

type WorkflowTask = OrchestrationStatus["tasks"][number];

interface WorkflowEventsPanelProps {
  task: WorkflowTask;
  workspace: WorkspaceSnapshot | null;
  workspaceError: string | null;
}

export function WorkflowEventsPanel({
  task,
  workspace,
  workspaceError,
}: WorkflowEventsPanelProps) {
  return (
    <div className="mt-6 space-y-4">
      <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
          Attempt History
        </div>
        <div className="mt-3 space-y-3">
          {task.attempts.map((attempt) => (
            <div
              key={`${task.task}-${attempt.attempt}`}
              className="rounded-2xl border border-slate-200 bg-white px-4 py-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                    attempt {attempt.attempt}
                  </span>
                  <WorkflowRunStatusBadge status={attempt.status} />
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
        {workspaceError && <div className="mt-3 text-sm text-rose-700">{workspaceError}</div>}
        {workspace?.auditEvents.length ? (
          <div className="mt-3">
            <PreviewRecordList
              items={workspace.auditEvents
                .slice()
                .sort((left, right) => right.recordedAtMs - left.recordedAtMs)}
              previewLimit={12}
              emptyState={null}
              getKey={(event) =>
                `${event.recordedAtMs}:${event.category}:${event.kind}:${event.detail}`
              }
              renderItem={(event) => <WorkflowRuntimeEventCard event={event} />}
              modalTitle="Runtime Events"
              modalDescription="Audit completo disponibile per il task selezionato."
            />
          </div>
        ) : (
          <div className="mt-3 text-sm text-slate-500">
            No runtime audit events available for the selected attempt.
          </div>
        )}
      </div>
    </div>
  );
}

function WorkflowRuntimeEventCard({
  event,
}: {
  event: NonNullable<WorkspaceSnapshot["auditEvents"]>[number];
}) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-white px-4 py-3">
      <div className="flex items-center justify-between gap-3">
        <div className="text-sm font-semibold text-slate-900">{event.title}</div>
        <div className="text-[11px] text-slate-500">{formatTimestamp(event.recordedAtMs)}</div>
      </div>
      <div className="mt-1 text-xs uppercase tracking-wider text-slate-400">
        {event.category} · {event.kind}
      </div>
      <div className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap text-sm leading-6 text-slate-600">
        {event.detail}
      </div>
    </div>
  );
}
