import { ArrowRight, RefreshCw } from "lucide-react";
import { Link } from "react-router-dom";

export function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

export function formatElapsed(seconds: number): string {
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

export function statusTone(status: string): string {
  switch (status) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "failed":
    case "cancelled":
      return "border-rose-200 bg-rose-50 text-rose-700";
    case "retry_wait":
    case "skipped":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "disabled":
      return "border-slate-200 bg-slate-100 text-slate-600";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

interface JobsControlsProps {
  liveRunsCount: number;
  finishedRunsCount: number;
  activeJobsCount: number;
  disabledJobsCount: number;
  actionError: string | null;
  onRefresh: () => void;
}

export function JobsControls({
  liveRunsCount,
  finishedRunsCount,
  activeJobsCount,
  disabledJobsCount,
  actionError,
  onRefresh,
}: JobsControlsProps) {
  return (
    <>
      <header className="rounded-[32px] border border-slate-200 bg-white px-8 py-8 shadow-sm">
        <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-3xl">
            <div className="text-xs font-bold uppercase tracking-[0.28em] text-slate-400">
              Jobs Runtime
            </div>
            <h1 className="mt-3 text-3xl font-bold tracking-tight text-slate-900">
              Live runs, run history and scheduled jobs
            </h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              This is the operational surface for workflow execution. Stop and delete
              runs here, inspect scheduled jobs here, and keep `Workflows` focused on
              design instead of monitoring.
            </p>
          </div>
          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              onClick={onRefresh}
              className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
            <Link
              to="/workflows"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Open Workflows
            </Link>
            <Link
              to="/control-center"
              className="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
            >
              Control Center
              <ArrowRight className="h-4 w-4" />
            </Link>
          </div>
        </div>

        <div className="mt-8 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <MetricCard label="Live Runs" value={liveRunsCount} />
          <MetricCard label="Run History" value={finishedRunsCount} />
          <MetricCard label="Active Jobs" value={activeJobsCount} />
          <MetricCard label="Disabled Jobs" value={disabledJobsCount} />
        </div>
      </header>

      {actionError && (
        <div className="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {actionError}
        </div>
      )}
    </>
  );
}

function MetricCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-slate-50 px-5 py-4">
      <div className="text-[11px] font-bold uppercase tracking-wider text-slate-400">
        {label}
      </div>
      <div className="mt-1 text-2xl font-bold text-slate-900">{value}</div>
    </div>
  );
}
