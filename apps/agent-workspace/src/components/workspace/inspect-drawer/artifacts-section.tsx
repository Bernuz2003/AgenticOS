import type { WorkspaceSnapshot } from "../../../lib/api";
import { InspectSection } from "./section-shell";

export function ArtifactsSection({ snapshot }: { snapshot: WorkspaceSnapshot | null }) {
  const accounting = snapshot?.accounting;
  const orchestration = snapshot?.orchestration;

  return (
    <InspectSection
      title="Artifacts"
      description="Session accounting and orchestration progress visible at a glance."
    >
      {accounting ? (
        <div className="grid grid-cols-2 gap-3 text-sm">
          <Metric label="Estimated cost" value={`$${accounting.estimatedCostUsd.toFixed(6)}`} />
          <Metric label="Requests" value={accounting.requestsTotal} />
          <Metric
            label="Tokens in / out"
            value={`${accounting.inputTokensTotal} / ${accounting.outputTokensTotal}`}
          />
          <Metric
            label="Errors"
            value={`${accounting.rateLimitErrors}/${accounting.authErrors}/${accounting.transportErrors}`}
          />
        </div>
      ) : (
        <div className="text-sm text-slate-500">No recorded accounting data.</div>
      )}

      {orchestration ? (
        <div className="mt-4 rounded-[20px] border border-slate-200 bg-slate-50 p-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-slate-950">
                Orchestration {orchestration.orchestrationId}
              </div>
              <div className="mt-1 text-xs text-slate-500">{orchestration.taskId}</div>
            </div>
            <span className="status-pill border border-slate-200 bg-white text-slate-700">
              {orchestration.policy}
            </span>
          </div>
          <div className="mt-4 grid grid-cols-4 gap-2 text-center text-sm">
            <Counter label="Run" value={orchestration.running} />
            <Counter label="Wait" value={orchestration.pending} />
            <Counter label="Done" value={orchestration.completed} />
            <Counter label="Fail" value={orchestration.failed} danger />
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

function Counter({
  label,
  value,
  danger = false,
}: {
  label: string;
  value: number;
  danger?: boolean;
}) {
  return (
    <div className="rounded-2xl border border-white bg-white px-2 py-3 shadow-sm">
      <div className="text-[10px] uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className={`mt-1 text-base font-semibold ${danger ? "text-rose-700" : "text-slate-950"}`}>
        {value}
      </div>
    </div>
  );
}
