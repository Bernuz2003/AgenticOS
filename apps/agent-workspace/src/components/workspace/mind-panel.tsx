import { useEffect, useRef, useState } from "react";
import { Activity, DatabaseZap, Sparkles, Waypoints, FileText, BarChart3 } from "lucide-react";
import { strategyLabel } from "../../lib/format";
import type { AgentSessionSummary } from "../../store/sessions-store";
import type { WorkspaceSnapshot } from "../../lib/api";

export function MindPanel({
  session,
  snapshot,
  loading,
  error,
  onOpenAudit,
}: {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  loading: boolean;
  error: string | null;
  onOpenAudit: () => void;
}) {
  const usedTokens = snapshot?.context?.contextTokensUsed ?? snapshot?.tokens ?? 0;
  const windowTokens = snapshot?.context?.contextWindowSize ?? snapshot?.maxTokens ?? 1;
  const progress = Math.min(100, Math.round((usedTokens / windowTokens) * 100));
  const strategy = snapshot?.context?.contextStrategy ?? session.contextStrategy;
  const compressions = snapshot?.context?.contextCompressions ?? 0;
  const retrievalHits = snapshot?.context?.contextRetrievalHits ?? 0;
  const backendClass = snapshot?.backendClass ?? "unknown";
  const contextSlotId = snapshot?.contextSlotId ?? null;
  const residentSlotState = snapshot?.residentSlotState ?? "unbound";
  const residentKv = snapshot?.backendCapabilities?.residentKv ?? false;
  const accounting = snapshot?.accounting ?? null;
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

  return (
    <aside className="bg-slate-50 border-l border-slate-200 flex min-h-[calc(100vh-2rem)] flex-col gap-6 p-6 overflow-y-auto w-full md:w-80 lg:w-96 flex-shrink-0">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-slate-900">
            Cognitive Telemetry
          </h2>
          <p className="text-xs font-semibold uppercase tracking-wider text-slate-500 mt-1">
            Real-time Analysis
          </p>
        </div>
        <button
          onClick={onOpenAudit}
          className="p-2 bg-white border border-slate-200 shadow-sm text-slate-600 rounded-lg hover:text-indigo-600 hover:border-indigo-200 hover:bg-indigo-50 transition-colors"
          title="Open Technical Audit"
        >
          <FileText className="w-5 h-5" />
        </button>
      </div>

      <section className="rounded-2xl bg-indigo-900 px-5 py-5 text-white shadow-sm relative overflow-hidden">
        <div className="absolute top-0 right-0 p-4 opacity-10">
          <BarChart3 className="w-24 h-24" />
        </div>
        <div className="relative z-10">
          <div className="flex items-center justify-between text-sm mb-3">
            <span className="text-indigo-200 font-medium">Context Horizon</span>
            <span className="font-bold">{usedTokens.toLocaleString()} / {windowTokens.toLocaleString()}</span>
          </div>
          <div className="h-2.5 w-full overflow-hidden rounded-full bg-indigo-950/50">
            <div 
              className="h-full rounded-full bg-gradient-to-r from-emerald-400 via-teal-400 to-cyan-400 transition-all duration-500" 
              style={{ width: `${progress}%` }} 
            />
          </div>
          <div className="mt-4 flex gap-4 text-xs">
            <div className="flex flex-col">
              <span className="text-indigo-300">Generated</span>
              <span className="font-semibold">{snapshot?.tokensGenerated ?? 0}</span>
            </div>
            <div className="flex flex-col">
              <span className="text-indigo-300">Syscalls</span>
              <span className="font-semibold">{snapshot?.syscallsUsed ?? 0}</span>
            </div>
          </div>
        </div>
      </section>

      <div className="space-y-4">
        <div className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-3 text-sm font-bold text-slate-900">
            <Waypoints className="h-4 w-4 text-indigo-500" />
            Strategy: {strategyLabel(strategy)}
          </div>
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span className="block text-slate-500 mb-0.5">Backend</span>
              <span className="font-medium text-slate-900">{backendClass}</span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Resident KV</span>
              <span className="font-medium text-slate-900">{residentKv ? "Yes" : "No"}</span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Slot ID</span>
              <span className="font-medium text-slate-900">{contextSlotId ?? "none"}</span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Slot State</span>
              <span className="font-medium text-slate-900 capitalize">{residentSlotState}</span>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm flex flex-col items-center justify-center text-center">
            <span className="text-[10px] uppercase font-bold tracking-widest text-slate-400 mb-1 flex items-center gap-1">
              <DatabaseZap className="w-3 h-3" />
              Compressions
            </span>
            <span className="text-2xl font-black text-slate-800">{compressions}</span>
          </div>
          <div className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm flex flex-col items-center justify-center text-center">
            <span className="text-[10px] uppercase font-bold tracking-widest text-slate-400 mb-1 flex items-center gap-1">
              <Activity className="w-3 h-3" />
              Retrievals
            </span>
            <span className="text-2xl font-black text-slate-800">{retrievalHits}</span>
          </div>
        </div>

        <div className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
          <div className="text-xs font-bold uppercase tracking-wider text-slate-500 mb-3 flex items-center gap-2">
            <BarChart3 className="w-4 h-4 text-indigo-400" />
            Session Accounting
          </div>
          {accounting ? (
            <div className="grid grid-cols-2 gap-3 text-sm">
               <div>
                 <span className="block text-slate-500 text-xs">Costs</span>
                 <span className="font-bold text-emerald-600">${accounting.estimatedCostUsd.toFixed(6)}</span>
               </div>
               <div>
                 <span className="block text-slate-500 text-xs">Requests</span>
                 <span className="font-bold text-slate-900">{accounting.requestsTotal}</span>
               </div>
               <div>
                 <span className="block text-slate-500 text-xs">Tokens IN/OUT</span>
                 <span className="font-semibold text-slate-700">{accounting.inputTokensTotal} / {accounting.outputTokensTotal}</span>
               </div>
               <div>
                 <span className="block text-slate-500 text-xs">Errors</span>
                 <span className="font-semibold text-rose-600">{accounting.rateLimitErrors}/{accounting.authErrors}/{accounting.transportErrors}</span>
               </div>
            </div>
          ) : (
            <div className="text-sm text-slate-500 italic text-center py-2">
              No recorded accounting data
            </div>
          )}
        </div>
      </div>

      {snapshot?.orchestration && (
        <section className="rounded-2xl border border-indigo-200 bg-indigo-50 p-4">
          <div className="font-bold text-indigo-900 text-sm mb-2 flex items-center justify-between">
            <span>Orchestration {snapshot.orchestration.orchestrationId}</span>
            <span className="status-pill bg-indigo-100 text-indigo-700 text-xs">{snapshot.orchestration.policy}</span>
          </div>
          <div className="text-sm text-indigo-800 mb-3 truncate">
            Task: <span className="font-semibold">{snapshot.orchestration.taskId}</span>
          </div>
          <div className="grid grid-cols-4 gap-2 text-center text-xs">
            <div className="bg-white rounded-lg py-2 shadow-sm font-semibold text-indigo-900 flex flex-col">
              <span className="text-[10px] text-indigo-400 uppercase">Run</span> {snapshot.orchestration.running}
            </div>
            <div className="bg-white rounded-lg py-2 shadow-sm font-semibold text-indigo-900 flex flex-col">
              <span className="text-[10px] text-indigo-400 uppercase">Wait</span> {snapshot.orchestration.pending}
            </div>
            <div className="bg-white rounded-lg py-2 shadow-sm font-semibold text-indigo-900 flex flex-col">
              <span className="text-[10px] text-indigo-400 uppercase">Done</span> {snapshot.orchestration.completed}
            </div>
            <div className="bg-white rounded-lg py-2 shadow-sm font-semibold text-rose-600 flex flex-col">
              <span className="text-[10px] text-rose-300 uppercase">Fail</span> {snapshot.orchestration.failed}
            </div>
          </div>
        </section>
      )}

      {compactionToast && (
        <div className="rounded-xl border border-amber-200 bg-amber-50 p-3 flex gap-3 animate-in slide-in-from-bottom-2 fade-in">
          <Sparkles className="h-5 w-5 text-amber-500 shrink-0" />
          <div className="text-sm text-amber-900">
            <strong>Context Compaction Alert</strong>
            <p className="mt-0.5 text-amber-800/80">{compactionToast}</p>
          </div>
        </div>
      )}

      {loading && <div className="text-xs text-slate-400 text-center animate-pulse">Syncing telemetry data...</div>}
      {error && <div className="rounded-xl bg-rose-50 border border-rose-100 p-3 text-sm text-rose-600">{error}</div>}
    </aside>
  );
}
