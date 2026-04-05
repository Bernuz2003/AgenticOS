import { RefreshCcw, Save } from "lucide-react";

import type { CoreDumpSummary } from "../../../lib/api";
import { formatTimestamp } from "../../../lib/workspace/format";

interface DumpExplorerProps {
  dumps: CoreDumpSummary[];
  selectedDumpId: string | null;
  loading: boolean;
  capturePending: boolean;
  activeReplaySourceDumpId: string | null;
  canCapture: boolean;
  onRefresh: () => void;
  onCapture: () => void;
  onSelect: (dumpId: string) => void;
}

export function DumpExplorer({
  dumps,
  selectedDumpId,
  loading,
  capturePending,
  activeReplaySourceDumpId,
  canCapture,
  onRefresh,
  onCapture,
  onSelect,
}: DumpExplorerProps) {
  return (
    <section className="panel-surface flex min-h-0 flex-col overflow-hidden">
      <div className="flex items-start justify-between gap-3 border-b border-slate-200 p-4">
        <div>
          <div className="text-lg font-semibold tracking-tight text-slate-950">Core Dumps</div>
          <p className="mt-1 text-sm text-slate-500">
            Dedicated explorer for captured session state and replay entry points.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onRefresh}
            className="inline-flex h-10 w-10 items-center justify-center rounded-2xl border border-slate-200 bg-white text-slate-600 transition hover:border-slate-300 hover:text-slate-950"
            title="Refresh core dumps"
          >
            <RefreshCcw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </button>
          <button
            type="button"
            onClick={onCapture}
            disabled={!canCapture || capturePending}
            className="inline-flex items-center gap-2 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-2 text-sm font-semibold text-amber-900 transition hover:border-amber-300 hover:bg-amber-100 disabled:cursor-not-allowed disabled:border-slate-200 disabled:bg-slate-100 disabled:text-slate-400"
          >
            <Save className="h-4 w-4" />
            {capturePending ? "Capturing..." : "Capture"}
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {dumps.length === 0 ? (
          <div className="rounded-[24px] border border-dashed border-slate-200 bg-slate-50 px-5 py-6 text-sm text-slate-500">
            No core dumps recorded yet for this session.
          </div>
        ) : (
          <div className="space-y-3">
            {dumps.map((dump, index) => {
              const selected = dump.dumpId === selectedDumpId;
              return (
                <div
                  key={dump.dumpId}
                  className={`rounded-[24px] border p-3 transition ${
                    selected
                      ? "border-amber-300 bg-amber-50"
                      : "border-slate-200 bg-white hover:border-slate-300"
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => onSelect(dump.dumpId)}
                    className="block w-full min-w-0 text-left"
                  >
                    <div className="truncate text-sm font-semibold text-slate-950">
                      {dump.reason}
                    </div>
                    <div className="mt-1 text-xs text-slate-500">
                      {formatTimestamp(dump.createdAtMs)} · PID {dump.pid ?? "n/a"}
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2">
                      <Badge label={dump.reason.startsWith("manual") ? "manual" : "auto"} />
                      <Badge label={dump.fidelity} />
                      {index === 0 ? <Badge label="latest" emphasis /> : null}
                      {activeReplaySourceDumpId === dump.dumpId ? (
                        <Badge label="replay launched" replay />
                      ) : null}
                    </div>
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}

function Badge({
  label,
  emphasis = false,
  replay = false,
}: {
  label: string;
  emphasis?: boolean;
  replay?: boolean;
}) {
  const tone = replay
    ? "border-emerald-200 bg-emerald-50 text-emerald-700"
    : emphasis
      ? "border-slate-950 bg-slate-950 text-white"
      : "border-slate-200 bg-slate-100 text-slate-600";

  return (
    <span className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.18em] ${tone}`}>
      {label}
    </span>
  );
}
