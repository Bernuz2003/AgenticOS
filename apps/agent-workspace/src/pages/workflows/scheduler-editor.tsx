import type { Dispatch, SetStateAction } from "react";

import type { JobTriggerKind, SchedulerDraft } from "../../lib/workflow-builder";

function formatDateTimeLocal(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

export function formatSchedulerSummary(schedulerDraft: SchedulerDraft): string {
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

interface SchedulerEditorProps {
  schedulerDraft: SchedulerDraft;
  setSchedulerDraft: Dispatch<SetStateAction<SchedulerDraft>>;
}

export function SchedulerEditor({
  schedulerDraft,
  setSchedulerDraft,
}: SchedulerEditorProps) {
  return (
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
  );
}
