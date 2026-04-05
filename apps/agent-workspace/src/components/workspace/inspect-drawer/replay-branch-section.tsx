import type { WorkspaceSnapshot } from "../../../lib/api";
import { InspectSection } from "./section-shell";

export function ReplayBranchSection({ snapshot }: { snapshot: WorkspaceSnapshot | null }) {
  const replay = snapshot?.replay;
  if (!replay) {
    return null;
  }

  return (
    <InspectSection
      title="Replay Branch"
      description="Source dump provenance and current divergence against the baseline."
    >
      <div className="grid grid-cols-2 gap-3 text-sm">
        <Metric label="Source dump" value={replay.sourceDumpId} />
        <Metric label="Replay mode" value={replay.replayMode} />
        <Metric label="Tool mode" value={replay.toolMode} />
        <Metric label="Initial state" value={replay.initialState} />
        <Metric label="Messages delta" value={signed(replay.diff.replayMessagesDelta)} />
        <Metric label="Tool delta" value={signed(replay.diff.toolInvocationsDelta)} />
        <Metric label="Branch-only messages" value={replay.diff.branchOnlyMessages} />
        <Metric label="Changed outputs" value={replay.diff.changedToolOutputs} />
      </div>
      {replay.diff.latestBranchMessage ? (
        <div className="mt-4 rounded-[20px] border border-sky-200 bg-sky-50 px-4 py-3 text-sm text-sky-950">
          <div className="font-semibold">Latest branch message</div>
          <div className="mt-1 whitespace-pre-wrap leading-relaxed">
            {replay.diff.latestBranchMessage}
          </div>
        </div>
      ) : null}
    </InspectSection>
  );
}

function Metric({ label, value }: { label: string; value: number | string }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className="mt-1 font-medium text-slate-900">{value}</div>
    </div>
  );
}

function signed(value: number | null): string {
  if (value === null) {
    return "n/a";
  }
  return value > 0 ? `+${value}` : String(value);
}
