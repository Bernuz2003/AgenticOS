import { GitBranch, ScanSearch, Waypoints } from "lucide-react";

import type { WorkspaceSnapshot } from "../../../lib/api";
import { formatValue } from "./format";

interface ReplayBranchCardProps {
  snapshot: WorkspaceSnapshot | null;
}

export function ReplayBranchCard({ snapshot }: ReplayBranchCardProps) {
  const replay = snapshot?.replay;
  if (!replay) {
    return null;
  }

  return (
    <section className="rounded-2xl border border-emerald-200 bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-bold text-slate-900">
            <GitBranch className="h-4 w-4 text-emerald-600" />
            Replay Branch Diff
          </div>
          <p className="mt-1 text-xs text-slate-500">
            Counterfactual branch provenance and divergence against the source dump.
          </p>
        </div>
        <span className="rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1 text-[11px] font-bold text-emerald-800">
          {replay.replayMode}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-3 text-xs">
        <Metric label="Source dump" value={replay.sourceDumpId} />
        <Metric label="Tool mode" value={replay.toolMode} />
        <Metric label="Initial state" value={replay.initialState} />
        <Metric label="Fidelity" value={replay.sourceFidelity} />
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3">
        <div className="rounded-xl border border-slate-200 bg-slate-50 p-3">
          <div className="mb-2 flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-500">
            <Waypoints className="h-3.5 w-3.5 text-indigo-500" />
            Source Baseline
          </div>
          <div className="space-y-1 text-xs text-slate-700">
            <div>Context segments: {formatValue(replay.baseline.sourceContextSegments)}</div>
            <div>Episodic segments: {formatValue(replay.baseline.sourceEpisodicSegments)}</div>
            <div>Replay messages: {formatValue(replay.baseline.sourceReplayMessages)}</div>
            <div>Tool invocations: {formatValue(replay.baseline.sourceToolInvocations)}</div>
          </div>
        </div>

        <div className="rounded-xl border border-slate-200 bg-slate-50 p-3">
          <div className="mb-2 flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-500">
            <ScanSearch className="h-3.5 w-3.5 text-amber-500" />
            Branch Delta
          </div>
          <div className="space-y-1 text-xs text-slate-700">
            <div>Messages delta: {signed(replay.diff.replayMessagesDelta)}</div>
            <div>Tool delta: {signed(replay.diff.toolInvocationsDelta)}</div>
            <div>Changed outputs: {formatValue(replay.diff.changedToolOutputs)}</div>
            <div>New tool calls: {formatValue(replay.diff.branchOnlyToolCalls)}</div>
          </div>
        </div>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
        <Metric
          label="Live context"
          value={
            replay.diff.currentContextSegments === null
              ? "offline"
              : signed(replay.diff.contextSegmentsDelta)
          }
        />
        <Metric
          label="Live episodic"
          value={
            replay.diff.currentEpisodicSegments === null
              ? "offline"
              : signed(replay.diff.episodicSegmentsDelta)
          }
        />
        <Metric label="Branch-only messages" value={replay.diff.branchOnlyMessages} />
        <Metric label="Completed tool calls" value={replay.diff.completedToolCalls} />
      </div>

      {replay.diff.latestBranchMessage && (
        <div className="mt-4 rounded-xl border border-sky-200 bg-sky-50 px-3 py-3 text-xs text-sky-900">
          <div className="font-semibold">Latest branch message</div>
          <div className="mt-1 whitespace-pre-wrap leading-relaxed">
            {replay.diff.latestBranchMessage}
          </div>
        </div>
      )}

      {replay.diff.invocationDiffs.length > 0 && (
        <div className="mt-4 rounded-2xl border border-slate-200 bg-slate-50 p-4">
          <div className="mb-3 text-xs font-bold uppercase tracking-wider text-slate-500">
            Invocation diff
          </div>
          <div className="space-y-3">
            {replay.diff.invocationDiffs.map((entry) => (
              <div
                key={`${entry.sourceToolCallId ?? "new"}:${entry.replayToolCallId ?? entry.commandText}`}
                className="rounded-xl border border-slate-200 bg-white px-3 py-3"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold text-slate-900">
                      {entry.toolName}
                    </div>
                    <div className="mt-1 break-all text-[11px] text-slate-500">
                      {entry.commandText}
                    </div>
                  </div>
                  <span
                    className={`rounded-full border px-2 py-1 text-[10px] font-bold ${
                      entry.branchOnly
                        ? "border-amber-200 bg-amber-50 text-amber-800"
                        : "border-rose-200 bg-rose-50 text-rose-700"
                    }`}
                  >
                    {entry.branchOnly ? "New call" : "Diverged"}
                  </span>
                </div>
                <div className="mt-3 grid grid-cols-2 gap-3 text-[11px]">
                  <div>
                    <span className="mb-0.5 block text-slate-500">Source</span>
                    <span className="font-medium text-slate-900">
                      {formatStatus(entry.sourceStatus, entry.sourceOutputText)}
                    </span>
                  </div>
                  <div>
                    <span className="mb-0.5 block text-slate-500">Replay</span>
                    <span className="font-medium text-slate-900">
                      {formatStatus(entry.replayStatus, entry.replayOutputText)}
                    </span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

function Metric({
  label,
  value,
}: {
  label: string;
  value: number | string | null | undefined;
}) {
  return (
    <div>
      <span className="mb-0.5 block text-slate-500">{label}</span>
      <span className="font-medium text-slate-900">{formatValue(value)}</span>
    </div>
  );
}

function signed(value: number | null): string {
  if (value === null) {
    return "n/a";
  }
  if (value > 0) {
    return `+${value}`;
  }
  return String(value);
}

function formatStatus(status: string | null, output: string | null): string {
  if (!status) {
    return "not replayed";
  }
  if (!output) {
    return status;
  }
  return `${status} · ${output}`;
}
