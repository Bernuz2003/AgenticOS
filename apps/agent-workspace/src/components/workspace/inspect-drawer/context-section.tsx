import { strategyLabel } from "../../../lib/utils/formatting";
import type { WorkspaceSnapshot } from "../../../lib/api";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import { formatLatencyMs } from "../../../lib/workspace/format";
import { InspectSection } from "./section-shell";

interface ContextSectionProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  compactionToast: string | null;
}

export function ContextSection({
  session,
  snapshot,
  compactionToast,
}: ContextSectionProps) {
  const context = snapshot?.context;
  const usedTokens = context?.contextTokensUsed ?? snapshot?.tokens ?? 0;
  const windowTokens = context?.contextWindowSize ?? snapshot?.maxTokens ?? 1;
  const progress = Math.min(100, Math.round((usedTokens / Math.max(windowTokens, 1)) * 100));

  return (
    <InspectSection
      title="Context"
      description="Strategy, compaction state and semantic retrieval summary."
    >
      <div className="rounded-[20px] border border-slate-200 bg-slate-50 p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-xs uppercase tracking-[0.18em] text-slate-400">Strategy</div>
            <div className="mt-1 text-sm font-semibold text-slate-950">
              {strategyLabel(context?.contextStrategy ?? session.contextStrategy)}
            </div>
          </div>
          <div className="text-right">
            <div className="text-xs uppercase tracking-[0.18em] text-slate-400">Usage</div>
            <div className="mt-1 text-sm font-semibold text-slate-950">
              {usedTokens.toLocaleString()} / {windowTokens.toLocaleString()}
            </div>
          </div>
        </div>
        <div className="mt-4 h-2 overflow-hidden rounded-full bg-slate-200">
          <div
            className="h-full rounded-full bg-slate-900 transition-all"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3 text-sm">
        <Metric label="Compressions" value={context?.contextCompressions ?? 0} />
        <Metric label="Retrieval hits" value={context?.contextRetrievalHits ?? 0} />
        <Metric label="Requests / misses" value={`${context?.contextRetrievalRequests ?? 0} / ${context?.contextRetrievalMisses ?? 0}`} />
        <Metric label="Context segments" value={context?.contextSegments ?? 0} />
        <Metric label="Episodic segments" value={context?.episodicSegments ?? 0} />
        <Metric
          label="Last retrieval"
          value={formatLatencyMs(context?.lastRetrievalLatencyMs ?? 0)}
        />
      </div>

      {compactionToast ? (
        <div className="mt-4 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900">
          <div className="font-semibold">Latest compaction</div>
          <div className="mt-1 leading-relaxed">{compactionToast}</div>
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
