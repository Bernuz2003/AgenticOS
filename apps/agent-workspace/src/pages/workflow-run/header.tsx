import { Link } from "react-router-dom";
import { PauseCircle, RefreshCw, Trash2 } from "lucide-react";

import type { OrchestrationStatus } from "../../lib/api";
import { WorkflowRunProgressBar } from "../../components/workflows/run/progress-bar";
import { formatElapsed, formatReasonLabel } from "./format";

interface WorkflowRunHeaderProps {
  detail: OrchestrationStatus;
  busyKey: string | null;
  progress: number;
  runTerminationReasons: string[];
  onReload: () => void | Promise<void>;
  onStop: () => void | Promise<void>;
  onDelete: () => void | Promise<void>;
}

export function WorkflowRunHeader({
  detail,
  busyKey,
  progress,
  runTerminationReasons,
  onReload,
  onStop,
  onDelete,
}: WorkflowRunHeaderProps) {
  return (
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
            onClick={() => void onReload()}
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
            onClick={() => void onStop()}
            disabled={detail.finished || busyKey === "stop"}
            className="inline-flex items-center gap-2 rounded-xl border border-amber-200 bg-amber-50 px-4 py-2.5 text-sm font-semibold text-amber-700 hover:bg-amber-100 disabled:opacity-40"
          >
            <PauseCircle className="h-4 w-4" />
            {busyKey === "stop" ? "Stopping..." : "Stop"}
          </button>
          <button
            type="button"
            onClick={() => void onDelete()}
            disabled={!detail.finished || busyKey === "delete"}
            className="inline-flex items-center gap-2 rounded-xl border border-rose-200 bg-rose-50 px-4 py-2.5 text-sm font-semibold text-rose-700 hover:bg-rose-100 disabled:opacity-40"
          >
            <Trash2 className="h-4 w-4" />
            {busyKey === "delete" ? "Deleting..." : "Delete"}
          </button>
        </div>
      </div>

      <WorkflowRunProgressBar value={progress} className="mt-8" />

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
  );
}
