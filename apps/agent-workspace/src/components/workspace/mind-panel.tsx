import { useEffect, useRef, useState } from "react";
import { Activity, DatabaseZap, Filter, Sparkles, Waypoints } from "lucide-react";
import { strategyLabel } from "../../lib/format";
import type { AgentSessionSummary } from "../../store/sessions-store";
import type { AuditEvent, WorkspaceSnapshot } from "../../lib/api";

export function MindPanel({
  session,
  snapshot,
  loading,
  error,
}: {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  loading: boolean;
  error: string | null;
}) {
  const usedTokens = snapshot?.context?.contextTokensUsed ?? snapshot?.tokens ?? 0;
  const windowTokens = snapshot?.context?.contextWindowSize ?? snapshot?.maxTokens ?? 1;
  const progress = Math.min(100, Math.round((usedTokens / windowTokens) * 100));
  const strategy = snapshot?.context?.contextStrategy ?? session.contextStrategy;
  const compressions = snapshot?.context?.contextCompressions ?? 0;
  const retrievalHits = snapshot?.context?.contextRetrievalHits ?? 0;
  const auditEvents = snapshot?.auditEvents ?? [
    {
      category: "status",
      title: "Context snapshot",
      detail: `pid=${session.pid} strategy=${strategy}`,
    },
  ];
  const [auditFilter, setAuditFilter] = useState<string>("all");
  const [compactionToast, setCompactionToast] = useState<string | null>(null);
  const lastCompactionRef = useRef<string | null>(null);

  useEffect(() => {
    const currentReason = snapshot?.context?.lastCompactionReason ?? null;
    if (
      currentReason &&
      currentReason !== lastCompactionRef.current
    ) {
      setCompactionToast(currentReason);
      const timeout = window.setTimeout(() => setCompactionToast(null), 4000);
      lastCompactionRef.current = currentReason;
      return () => window.clearTimeout(timeout);
    }

    lastCompactionRef.current = currentReason;
    return undefined;
  }, [snapshot?.context?.lastCompactionReason]);

  const filteredAuditEvents = auditEvents.filter((event) => {
    if (auditFilter === "all") {
      return true;
    }
    return event.category === auditFilter;
  });

  const filterOptions: Array<{ value: string; label: string }> = [
    { value: "all", label: "All" },
    { value: "runtime", label: "Runtime" },
    { value: "orchestration", label: "Orch" },
    { value: "status", label: "Status" },
    { value: "compaction", label: "Compaction" },
    { value: "summary", label: "Summary" },
  ];

  return (
    <aside className="panel-surface flex min-h-[680px] flex-col gap-5 p-5">
      <div>
        <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
          Mind Panel
        </p>
        <h2 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
          Telemetria cognitiva
        </h2>
      </div>

      <section className="rounded-[24px] bg-slate-950 px-5 py-5 text-white">
        <div className="flex items-center justify-between text-sm">
          <span>Budget contesto</span>
          <span className="font-semibold">{usedTokens} / {windowTokens}</span>
        </div>
        <div className="mt-4 h-3 overflow-hidden rounded-full bg-white/15">
          <div className="h-full rounded-full bg-gradient-to-r from-amber-300 via-orange-300 to-emerald-300" style={{ width: `${progress}%` }} />
        </div>
        <div className="mt-3 text-xs text-white/70">
          {snapshot ? `tokens_generated=${snapshot.tokensGenerated} syscalls_used=${snapshot.syscallsUsed}` : "Waiting for STATUS <pid> snapshot"}
        </div>
      </section>

      <div className="grid gap-3">
        <div className="rounded-[24px] bg-white p-4">
          <div className="flex items-center gap-3 text-sm font-semibold text-slate-900">
            <Waypoints className="h-4 w-4" />
            Strategy: {strategyLabel(strategy)}
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="rounded-[24px] bg-white p-4">
            <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">
              <DatabaseZap className="h-4 w-4" />
              Compressions
            </div>
            <div className="mt-3 text-3xl font-bold text-slate-950">{compressions}</div>
          </div>
          <div className="rounded-[24px] bg-white p-4">
            <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">
              <Activity className="h-4 w-4" />
              Retrieval hits
            </div>
            <div className="mt-3 text-3xl font-bold text-slate-950">{retrievalHits}</div>
          </div>
        </div>
      </div>

      {snapshot?.orchestration ? (
        <section className="rounded-[24px] border border-cyan-200 bg-cyan-50 px-4 py-4 text-sm text-cyan-950">
          <div className="font-semibold uppercase tracking-[0.18em] text-cyan-700">
            Orchestration {snapshot.orchestration.orchestrationId}
          </div>
          <div className="mt-2 text-sm">
            Task corrente: <span className="font-semibold">{snapshot.orchestration.taskId}</span>
          </div>
          <div className="mt-3 grid grid-cols-4 gap-2 text-xs text-cyan-900">
            <div className="rounded-2xl bg-white/70 p-3">run {snapshot.orchestration.running}</div>
            <div className="rounded-2xl bg-white/70 p-3">pending {snapshot.orchestration.pending}</div>
            <div className="rounded-2xl bg-white/70 p-3">done {snapshot.orchestration.completed}</div>
            <div className="rounded-2xl bg-white/70 p-3">failed {snapshot.orchestration.failed}</div>
          </div>
        </section>
      ) : null}

      {compactionToast ? (
        <section className="rounded-[24px] border border-amber-300 bg-amber-50 px-4 py-4 text-sm text-amber-900">
          <div className="flex items-center gap-2 font-semibold">
            <Sparkles className="h-4 w-4" />
            Nuovo compaction event
          </div>
          <p className="mt-2">{compactionToast}</p>
        </section>
      ) : null}

      {loading ? <div className="text-xs text-slate-500">Aggiornamento telemetria in corso...</div> : null}
      {error ? <div className="rounded-2xl bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</div> : null}

      <section className="flex flex-1 flex-col rounded-[24px] border border-slate-900/8 bg-white p-4">
        <div className="flex items-center justify-between gap-3">
          <div className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">
            Audit stream
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {filterOptions.map((option) => (
              <button
                key={option.value}
                onClick={() => setAuditFilter(option.value)}
                className={`rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] transition ${
                  auditFilter === option.value
                    ? "bg-slate-950 text-white"
                    : "bg-slate-100 text-slate-600 hover:bg-slate-200"
                }`}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
        <div className="mt-4 flex-1 space-y-3 overflow-auto rounded-2xl bg-slate-950 p-4 text-[12px] leading-6 text-emerald-100">
          {filteredAuditEvents.map((event: AuditEvent, index) => (
            <div key={`${event.category}-${event.title}-${index}`} className="rounded-2xl border border-white/8 bg-white/5 p-3">
              <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-emerald-300">
                {event.category}
              </div>
              <div className="mt-1 font-semibold text-white">{event.title}</div>
              <div className="mt-1 font-mono text-emerald-100/90">{event.detail}</div>
            </div>
          ))}
          {filteredAuditEvents.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-white/10 p-3 text-emerald-100/70">
              Nessun evento per il filtro selezionato.
            </div>
          ) : null}
        </div>
      </section>
    </aside>
  );
}