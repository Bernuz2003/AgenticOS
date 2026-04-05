import { friendlyRuntimeLabel } from "../../../lib/models/labels";
import { runtimeStateLabel, runtimeStateTone } from "../../../lib/utils/formatting";
import type { WorkspaceSnapshot } from "../../../lib/api";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import { formatLatencyMs, formatWorkspaceValue } from "../../../lib/workspace/format";
import { InspectSection } from "./section-shell";

interface RuntimeSectionProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
}

export function RuntimeSection({ session, snapshot }: RuntimeSectionProps) {
  const runtimeState = snapshot?.state ?? session.runtimeState ?? null;
  const runtimeLabel = friendlyRuntimeLabel(
    snapshot?.runtimeLabel ?? session.runtimeLabel ?? null,
    snapshot?.runtimeId ?? session.runtimeId ?? null,
  );

  return (
    <InspectSection
      title="Runtime"
      description="Live runtime state, quotas and resident context binding."
    >
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-950">{runtimeLabel}</div>
          <div className="mt-1 text-xs text-slate-500">
            PID {snapshot?.activePid ?? session.activePid ?? session.pid} · backend{" "}
            {snapshot?.backendClass ?? "n/a"}
          </div>
        </div>
        <span className={`status-pill border ${runtimeStateTone(runtimeState)}`}>
          {runtimeStateLabel(runtimeState)}
        </span>
      </div>

      <div className="grid grid-cols-2 gap-3 text-sm">
        <Metric label="Owner" value={snapshot?.ownerId} />
        <Metric label="Priority" value={snapshot?.priority} />
        <Metric label="Token cursor" value={snapshot?.indexPos} />
        <Metric label="Elapsed" value={formatLatencyMs(snapshot?.elapsedSecs ? snapshot.elapsedSecs * 1000 : null)} />
        <Metric label="Quota tokens" value={snapshot?.quotaTokens ?? "No limit"} />
        <Metric label="Quota syscalls" value={snapshot?.quotaSyscalls ?? "No limit"} />
        <Metric label="Context slot" value={snapshot?.contextSlotId} />
        <Metric label="Slot state" value={snapshot?.residentSlotState} />
      </div>
    </InspectSection>
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
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className="mt-1 font-medium text-slate-900">{formatWorkspaceValue(value)}</div>
    </div>
  );
}
