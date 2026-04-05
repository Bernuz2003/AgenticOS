import { GitBranch, RotateCcw } from "lucide-react";

import type { CoreDumpInfo, WorkspaceReplayDebugSnapshot } from "../../../lib/api";

interface ReplayTabProps {
  info: CoreDumpInfo | null;
  branchLabel: string;
  replayPending: boolean;
  currentReplay: WorkspaceReplayDebugSnapshot | null;
  onBranchLabelChange: (value: string) => void;
  onReplay: () => void;
}

export function ReplayTab({
  info,
  branchLabel,
  replayPending,
  currentReplay,
  onBranchLabelChange,
  onReplay,
}: ReplayTabProps) {
  if (!info) {
    return <div className="text-sm text-slate-500">Select a dump to configure replay.</div>;
  }

  return (
    <div className="space-y-4">
      <section className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold text-slate-950">Replay / Fork</h3>
            <p className="mt-1 text-sm text-slate-500">
              Launch a replay branch from this dump. Fork remains reserved until the backend
              exposes explicit counterfactual patching.
            </p>
          </div>
          <span className="status-pill border border-slate-200 bg-white text-slate-700">
            replay-ready
          </span>
        </div>

        <label className="mt-5 block">
          <span className="text-xs uppercase tracking-[0.18em] text-slate-400">Branch label</span>
          <input
            type="text"
            value={branchLabel}
            onChange={(event) => onBranchLabelChange(event.target.value)}
            placeholder={`replay-${info.dump.dumpId.slice(0, 8)}`}
            className="mt-2 w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-900 outline-none transition focus:border-slate-400"
          />
        </label>

        <div className="mt-4 grid gap-3 md:grid-cols-2">
          <StatusCard label="Tool mode" value="derived from backend replay defaults" />
          <StatusCard label="Context patching" value="not yet available in GUI" />
          <StatusCard label="Stub overrides" value="reserved for future fork mode" />
          <StatusCard label="Current action" value="launch replay branch and open workspace" />
        </div>

        <div className="mt-5 flex flex-wrap gap-3">
          <button
            type="button"
            onClick={onReplay}
            disabled={replayPending}
            className="inline-flex items-center gap-2 rounded-2xl bg-slate-950 px-4 py-3 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:bg-slate-300"
          >
            <RotateCcw className="h-4 w-4" />
            {replayPending ? "Launching replay..." : "Launch Replay"}
          </button>
          <button
            type="button"
            disabled
            className="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm font-semibold text-slate-400"
            title="Fork UI is reserved until backend support is exposed."
          >
            <GitBranch className="h-4 w-4" />
            Fork From This Dump
          </button>
        </div>
      </section>

      {currentReplay ? (
        <section className="rounded-[24px] border border-emerald-200 bg-emerald-50 p-4">
          <div className="text-sm font-semibold text-emerald-950">Current Session Replay Link</div>
          <div className="mt-3 grid gap-3 md:grid-cols-2">
            <StatusCard label="Source dump" value={currentReplay.sourceDumpId} />
            <StatusCard label="Replay mode" value={currentReplay.replayMode} />
            <StatusCard label="Tool mode" value={currentReplay.toolMode} />
            <StatusCard label="Initial state" value={currentReplay.initialState} />
          </div>
        </section>
      ) : null}
    </div>
  );
}

function StatusCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[20px] border border-slate-200 bg-white px-4 py-3">
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className="mt-2 text-sm font-medium text-slate-950">{value}</div>
    </div>
  );
}
