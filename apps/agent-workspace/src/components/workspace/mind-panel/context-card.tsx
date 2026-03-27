import {
  Activity,
  BarChart3,
  DatabaseZap,
  Sparkles,
  Waypoints,
} from "lucide-react";

import type { WorkspaceSnapshot } from "../../../lib/api";
import { strategyLabel } from "../../../lib/utils/formatting";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import { formatLatency } from "./format";

interface ContextCardProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  compactionToast: string | null;
}

export function ContextCard({
  session,
  snapshot,
  compactionToast,
}: ContextCardProps) {
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
  const retrieveMaxSegmentChars = snapshot?.context?.retrieveMaxSegmentChars ?? 0;
  const retrieveMinScore = snapshot?.context?.retrieveMinScore ?? 0;
  const backendClass = snapshot?.backendClass ?? "unknown";
  const pendingHumanRequest = snapshot?.pendingHumanRequest ?? null;
  const residentKv = snapshot?.backendCapabilities?.residentKv ?? false;
  const contextSlotId = snapshot?.contextSlotId ?? null;
  const residentSlotState = snapshot?.residentSlotState ?? "unbound";

  return (
    <>
      <section className="relative overflow-hidden rounded-2xl bg-indigo-900 px-5 py-5 text-white shadow-sm">
        <div className="absolute right-0 top-0 p-4 opacity-10">
          <BarChart3 className="h-24 w-24" />
        </div>
        <div className="relative z-10">
          <div className="mb-3 flex items-center justify-between text-sm">
            <span className="font-medium text-indigo-200">Context Horizon</span>
            <span className="font-bold">
              {usedTokens.toLocaleString()} / {windowTokens.toLocaleString()}
            </span>
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

      <div className="space-y-4">
        <div className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="mb-3 flex items-center gap-2 text-sm font-bold text-slate-900">
            <Waypoints className="h-4 w-4 text-indigo-500" />
            Strategy: {strategyLabel(strategy)}
          </div>
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span className="mb-0.5 block text-slate-500">Backend</span>
              <span className="font-medium text-slate-900">{backendClass}</span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Resident KV</span>
              <span className="font-medium text-slate-900">{residentKv ? "Yes" : "No"}</span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Slot ID</span>
              <span className="font-medium text-slate-900">{contextSlotId ?? "none"}</span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Slot State</span>
              <span className="font-medium capitalize text-slate-900">{residentSlotState}</span>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div className="flex flex-col items-center justify-center rounded-2xl border border-slate-200 bg-white p-4 text-center shadow-sm">
            <span className="mb-1 flex items-center gap-1 text-[10px] font-bold uppercase tracking-widest text-slate-400">
              <DatabaseZap className="h-3 w-3" />
              Compressions
            </span>
            <span className="text-2xl font-black text-slate-800">{compressions}</span>
          </div>
          <div className="flex flex-col items-center justify-center rounded-2xl border border-slate-200 bg-white p-4 text-center shadow-sm">
            <span className="mb-1 flex items-center gap-1 text-[10px] font-bold uppercase tracking-widest text-slate-400">
              <Activity className="h-3 w-3" />
              Retrieval Hits
            </span>
            <span className="text-2xl font-black text-slate-800">{retrievalHits}</span>
          </div>
        </div>

        <div className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
          <div className="mb-3 flex items-center gap-2 text-sm font-bold text-slate-900">
            <DatabaseZap className="h-4 w-4 text-indigo-500" />
            Semantic Retrieval
          </div>
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span className="mb-0.5 block text-slate-500">Episodic Segments</span>
              <span className="font-medium text-slate-900">{episodicSegments}</span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Episodic Tokens</span>
              <span className="font-medium text-slate-900">{episodicTokens}</span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Requests / Misses</span>
              <span className="font-medium text-slate-900">
                {retrievalRequests} / {retrievalMisses}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Candidates / Selected</span>
              <span className="font-medium text-slate-900">
                {retrievalCandidatesScored} / {retrievalSegmentsSelected}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Last Scan</span>
              <span className="font-medium text-slate-900">
                {lastRetrievalCandidatesScored} scored / {lastRetrievalSegmentsSelected} kept
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Latency / Top Score</span>
              <span className="font-medium text-slate-900">
                {formatLatency(lastRetrievalLatencyMs)} /{" "}
                {lastRetrievalTopScore === null ? "n/a" : lastRetrievalTopScore.toFixed(3)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Top K / Candidate Limit</span>
              <span className="font-medium text-slate-900">
                {retrieveTopK} / {retrieveCandidateLimit}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Min Score / Max Chars</span>
              <span className="font-medium text-slate-900">
                {retrieveMinScore.toFixed(2)} / {retrieveMaxSegmentChars}
              </span>
            </div>
          </div>
        </div>
      </div>

      {compactionToast && (
        <div className="animate-in slide-in-from-bottom-2 fade-in flex gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3">
          <Sparkles className="h-5 w-5 shrink-0 text-amber-500" />
          <div className="text-sm text-amber-900">
            <strong>Context Compaction Alert</strong>
            <p className="mt-0.5 text-amber-800/80">{compactionToast}</p>
          </div>
        </div>
      )}
    </>
  );
}
