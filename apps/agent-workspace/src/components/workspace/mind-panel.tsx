import { useEffect, useRef, useState } from "react";
import {
  Activity,
  BarChart3,
  Cloud,
  Cpu,
  DatabaseZap,
  FileText,
  Sparkles,
  Waypoints,
  Wrench,
} from "lucide-react";
import { runtimeStateLabel, runtimeStateTone, strategyLabel } from "../../lib/format";
import { friendlyRuntimeLabel } from "../../lib/model-labels";
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
  const retrievalRequests = snapshot?.context?.contextRetrievalRequests ?? 0;
  const retrievalMisses = snapshot?.context?.contextRetrievalMisses ?? 0;
  const retrievalCandidatesScored =
    snapshot?.context?.contextRetrievalCandidatesScored ?? 0;
  const retrievalSegmentsSelected =
    snapshot?.context?.contextRetrievalSegmentsSelected ?? 0;
  const lastRetrievalCandidatesScored =
    snapshot?.context?.lastRetrievalCandidatesScored ?? 0;
  const lastRetrievalSegmentsSelected =
    snapshot?.context?.lastRetrievalSegmentsSelected ?? 0;
  const lastRetrievalLatencyMs = snapshot?.context?.lastRetrievalLatencyMs ?? 0;
  const lastRetrievalTopScore = snapshot?.context?.lastRetrievalTopScore ?? null;
  const episodicSegments = snapshot?.context?.episodicSegments ?? 0;
  const episodicTokens = snapshot?.context?.episodicTokens ?? 0;
  const retrieveTopK = snapshot?.context?.retrieveTopK ?? 0;
  const retrieveCandidateLimit = snapshot?.context?.retrieveCandidateLimit ?? 0;
  const retrieveMaxSegmentChars =
    snapshot?.context?.retrieveMaxSegmentChars ?? 0;
  const retrieveMinScore = snapshot?.context?.retrieveMinScore ?? 0;
  const backendClass = snapshot?.backendClass ?? "unknown";
  const runtimeState = snapshot?.state ?? session.runtimeState ?? null;
  const runtimeLabel = friendlyRuntimeLabel(
    snapshot?.runtimeLabel ?? session.runtimeLabel ?? null,
    snapshot?.runtimeId ?? session.runtimeId ?? null,
  );
  const ownerId = snapshot?.ownerId ?? null;
  const toolCaller = snapshot?.toolCaller ?? null;
  const pendingHumanRequest = snapshot?.pendingHumanRequest ?? null;
  const indexPos = snapshot?.indexPos ?? null;
  const priority = snapshot?.priority ?? null;
  const quotaTokens = snapshot?.quotaTokens ?? null;
  const quotaSyscalls = snapshot?.quotaSyscalls ?? null;
  const contextSlotId = snapshot?.contextSlotId ?? null;
  const residentSlotState = snapshot?.residentSlotState ?? "unbound";
  const residentKv = snapshot?.backendCapabilities?.residentKv ?? false;
  const accounting = snapshot?.accounting ?? null;
  const permissions = snapshot?.permissions ?? null;
  const auditEvents = [...(snapshot?.auditEvents ?? [])].sort(
    (left, right) => right.recordedAtMs - left.recordedAtMs,
  );
  const [compactionToast, setCompactionToast] = useState<string | null>(null);
  const lastCompactionRef = useRef<string | null>(null);
  const diagnosticGroups = [
    {
      key: "tool",
      label: "Tools",
      icon: Wrench,
      events: auditEvents.filter((event) => event.category === "tool"),
    },
    {
      key: "remote",
      label: "Remote",
      icon: Cloud,
      events: auditEvents.filter((event) => event.category === "remote"),
    },
    {
      key: "process",
      label: "Process",
      icon: Activity,
      events: auditEvents.filter((event) => event.category === "process"),
    },
    {
      key: "runtime",
      label: "Runtime",
      icon: Cpu,
      events: auditEvents.filter(
        (event) => event.category === "runtime" || event.category === "admission",
      ),
    },
  ];
  const recentDiagnosticEvents = auditEvents.slice(0, 3);

  function formatValue(value: number | string | null | undefined): string {
    if (value === null || value === undefined || value === "") {
      return "n/a";
    }
    if (typeof value === "number") {
      return value.toLocaleString();
    }
    return value;
  }

  function formatLatency(ms: number): string {
    if (!Number.isFinite(ms) || ms <= 0) {
      return "0 ms";
    }
    if (ms < 1000) {
      return `${ms} ms`;
    }
    return `${(ms / 1000).toFixed(2)} s`;
  }

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
    <aside className="flex h-full min-h-0 w-full flex-shrink-0 flex-col gap-6 overflow-y-auto border-l border-slate-200 bg-slate-50 p-6 md:w-80 lg:w-96">
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

      {pendingHumanRequest && (
        <section className="rounded-2xl border border-sky-200 bg-sky-50 p-4 shadow-sm">
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-xs font-bold uppercase tracking-wider text-sky-600">
                Human-in-the-Loop
              </div>
              <div className="mt-1 text-sm font-semibold text-slate-900">
                {pendingHumanRequest.kind === "approval"
                  ? "Approval pending"
                  : "Human response pending"}
              </div>
            </div>
            <span className="rounded-full border border-sky-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-sky-700">
              Waiting
            </span>
          </div>
          <div className="mt-3 text-sm leading-relaxed text-slate-800">
            {pendingHumanRequest.question}
          </div>
          {pendingHumanRequest.choices.length > 0 && (
            <div className="mt-3 text-xs text-slate-600">
              Choices: {pendingHumanRequest.choices.join(", ")}
            </div>
          )}
          <div className="mt-2 text-xs text-slate-500">
            {pendingHumanRequest.allowFreeText
              ? "Free text enabled"
              : "Structured reply only"}
          </div>
        </section>
      )}

      <section className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
        <div className="flex items-center justify-between gap-3 mb-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
              Runtime Control
            </div>
            <div className="text-sm font-semibold text-slate-900 mt-1">
              {formatValue(runtimeLabel)}
            </div>
          </div>
          <span
            className={`rounded-full border px-3 py-1 text-[11px] font-bold ${runtimeStateTone(runtimeState)}`}
          >
            {runtimeStateLabel(runtimeState)}
          </span>
        </div>
        <div className="grid grid-cols-2 gap-3 text-xs">
          <div>
            <span className="block text-slate-500 mb-0.5">Priority</span>
            <span className="font-medium text-slate-900 capitalize">
              {formatValue(priority)}
            </span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Owner</span>
            <span className="font-medium text-slate-900">{formatValue(ownerId)}</span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Token Cursor</span>
            <span className="font-medium text-slate-900">{formatValue(indexPos)}</span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Quota Tokens</span>
            <span className="font-medium text-slate-900">{formatValue(quotaTokens)}</span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Quota Syscalls</span>
            <span className="font-medium text-slate-900">{formatValue(quotaSyscalls)}</span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Backend</span>
            <span className="font-medium text-slate-900">{backendClass}</span>
          </div>
        </div>
      </section>

      <section className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
        <div className="flex items-center justify-between gap-3 mb-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
              Process Permissions
            </div>
            <div className="text-sm font-semibold text-slate-900 mt-1">
              {permissions?.trustScope ?? "unknown"}
            </div>
          </div>
          <span
            className={`rounded-full border px-3 py-1 text-[11px] font-bold ${
              permissions?.actionsAllowed
                ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                : "border-slate-200 bg-slate-50 text-slate-600"
            }`}
          >
            {permissions?.actionsAllowed ? "Actions enabled" : "Actions blocked"}
          </span>
        </div>
        <div className="grid grid-cols-1 gap-3 text-xs">
          <div>
            <span className="block text-slate-500 mb-0.5">Caller</span>
            <span className="font-medium text-slate-900">{formatValue(toolCaller)}</span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Path Scopes</span>
            <span className="font-medium text-slate-900">
              {permissions && permissions.pathScopes.length > 0
                ? permissions.pathScopes.join(", ")
                : "none"}
            </span>
          </div>
          <div>
            <span className="block text-slate-500 mb-0.5">Allowed Tools</span>
            <span className="font-medium text-slate-900">
              {permissions && permissions.allowedTools.length > 0
                ? permissions.allowedTools.join(", ")
                : "none"}
            </span>
          </div>
        </div>
      </section>

      <section className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
        <div className="flex items-center justify-between gap-3 mb-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
              Diagnostics Overview
            </div>
            <div className="text-sm font-semibold text-slate-900 mt-1">
              {auditEvents.length} live audit events visible
            </div>
          </div>
          <button
            onClick={onOpenAudit}
            className="text-xs font-semibold text-indigo-600 hover:text-indigo-700"
          >
            Open full panel
          </button>
        </div>
        <div className="grid grid-cols-2 gap-3">
          {diagnosticGroups.map((group) => {
            const Icon = group.icon;
            const latest = group.events[0] ?? null;
            return (
              <div
                key={group.key}
                className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-500">
                    <Icon className="h-3.5 w-3.5 text-indigo-500" />
                    {group.label}
                  </div>
                  <span className="rounded-full bg-white px-2 py-0.5 text-[10px] font-bold text-slate-600 border border-slate-200">
                    {group.events.length}
                  </span>
                </div>
                <div className="mt-2 text-sm font-semibold text-slate-900">
                  {latest ? latest.title : "No events yet"}
                </div>
                <div className="mt-1 text-xs text-slate-500">
                  {latest
                    ? `${latest.kind} at ${new Date(latest.recordedAtMs).toLocaleTimeString()}`
                    : "Awaiting diagnostics"}
                </div>
              </div>
            );
          })}
        </div>
        <div className="mt-4 space-y-2">
          {recentDiagnosticEvents.length === 0 ? (
            <div className="text-xs text-slate-500">Nessun evento tecnico recente.</div>
          ) : (
            recentDiagnosticEvents.map((event, index) => (
              <div
                key={`${event.recordedAtMs}-${event.category}-${index}`}
                className="rounded-xl border border-slate-100 bg-white px-3 py-3"
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-semibold text-slate-900">{event.title}</div>
                  <div className="text-[10px] uppercase tracking-wider font-bold text-slate-500">
                    {event.category}
                  </div>
                </div>
                <div className="mt-1 text-xs text-slate-500">
                  {event.kind} at {new Date(event.recordedAtMs).toLocaleTimeString()}
                </div>
              </div>
            ))
          )}
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
              Retrieval Hits
            </span>
            <span className="text-2xl font-black text-slate-800">{retrievalHits}</span>
          </div>
        </div>

        <div className="rounded-2xl bg-white border border-slate-200 p-4 shadow-sm">
          <div className="flex items-center gap-2 mb-3 text-sm font-bold text-slate-900">
            <DatabaseZap className="h-4 w-4 text-indigo-500" />
            Semantic Retrieval
          </div>
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span className="block text-slate-500 mb-0.5">Episodic Segments</span>
              <span className="font-medium text-slate-900">{episodicSegments}</span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Episodic Tokens</span>
              <span className="font-medium text-slate-900">{episodicTokens}</span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Requests / Misses</span>
              <span className="font-medium text-slate-900">
                {retrievalRequests} / {retrievalMisses}
              </span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Candidates / Selected</span>
              <span className="font-medium text-slate-900">
                {retrievalCandidatesScored} / {retrievalSegmentsSelected}
              </span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Last Scan</span>
              <span className="font-medium text-slate-900">
                {lastRetrievalCandidatesScored} scored / {lastRetrievalSegmentsSelected} kept
              </span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Latency / Top Score</span>
              <span className="font-medium text-slate-900">
                {formatLatency(lastRetrievalLatencyMs)} /{" "}
                {lastRetrievalTopScore === null ? "n/a" : lastRetrievalTopScore.toFixed(3)}
              </span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Top K / Candidate Limit</span>
              <span className="font-medium text-slate-900">
                {retrieveTopK} / {retrieveCandidateLimit}
              </span>
            </div>
            <div>
              <span className="block text-slate-500 mb-0.5">Min Score / Max Chars</span>
              <span className="font-medium text-slate-900">
                {retrieveMinScore.toFixed(2)} / {retrieveMaxSegmentChars}
              </span>
            </div>
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
