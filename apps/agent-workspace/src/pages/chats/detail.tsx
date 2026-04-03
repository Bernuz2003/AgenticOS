import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { MindPanel } from "../../components/workspace/mind-panel";
import { TimelinePane } from "../../components/workspace/timeline-pane";
import { AuditDrawer } from "../../components/diagnostics/event-log";
import { ArrowLeft } from "lucide-react";
import {
  continueSessionOutput,
  sendSessionInput,
  stopSessionOutput,
} from "../../lib/api";
import { useSessionsStore } from "../../store/sessions-store";
import { useWorkspaceStore } from "../../store/workspace-store";
import { deriveSessionStatus, runtimeStateLabel } from "../../lib/utils/formatting";
import type { AgentSessionSummary } from "../../store/sessions-store";

function buildSyntheticSession(
  sessionId: string,
  pid: number,
  promptPreview: string,
  uptimeLabel: string,
  status: AgentSessionSummary["status"],
  snapshot: ReturnType<typeof useWorkspaceStore.getState>["snapshot"],
  timeline: ReturnType<typeof useWorkspaceStore.getState>["timeline"],
): AgentSessionSummary {
  return {
    sessionId,
    pid,
    activePid: snapshot?.activePid ?? null,
    lastPid: snapshot?.lastPid ?? (timeline?.pid ?? null),
    title: snapshot?.title ?? `Session ${sessionId}`,
    promptPreview,
    status,
    runtimeState: snapshot?.state ?? null,
    uptimeLabel,
    tokensLabel: snapshot ? String(snapshot.tokensGenerated) : "0",
    contextStrategy: snapshot?.context?.contextStrategy ?? "sliding_window",
    runtimeId: snapshot?.runtimeId ?? null,
    runtimeLabel: snapshot?.runtimeLabel ?? null,
    backendClass: snapshot?.backendClass ?? null,
  };
}

export function WorkspacePage() {
  const { sessionId } = useParams();
  const listedSession = useSessionsStore((state) =>
    state.sessions.find((item) => item.sessionId === sessionId),
  );
  const snapshot = useWorkspaceStore((state) => state.snapshot);
  const timeline = useWorkspaceStore((state) => state.timeline);
  const loading = useWorkspaceStore((state) => state.loading);
  const timelineLoading = useWorkspaceStore((state) => state.timelineLoading);
  const error = useWorkspaceStore((state) => state.error);
  const timelineError = useWorkspaceStore((state) => state.timelineError);
  const refresh = useWorkspaceStore((state) => state.refresh);
  const refreshTimeline = useWorkspaceStore((state) => state.refreshTimeline);
  const clear = useWorkspaceStore((state) => state.clear);
  const [composerValue, setComposerValue] = useState("");
  const [composerLoading, setComposerLoading] = useState(false);
  const [composerError, setComposerError] = useState<string | null>(null);
  const [turnActionLoading, setTurnActionLoading] = useState(false);
  const [turnActionError, setTurnActionError] = useState<string | null>(null);
  const [stopRequestPending, setStopRequestPending] = useState(false);
  const [auditOpen, setAuditOpen] = useState(false);

  const routePid =
    sessionId && sessionId.startsWith("pid-") ? Number(sessionId.slice(4)) : Number.NaN;
  const session = useMemo(() => {
    if (!sessionId) {
      return undefined;
    }

    if (listedSession) {
      return listedSession;
    }

    const derivedStatus = deriveSessionStatus(
      snapshot?.state,
      Boolean(timeline?.running),
    );

    if (!Number.isNaN(routePid)) {
      const liveSessionId = sessionId ?? `pid-${routePid}`;
      const synthetic = buildSyntheticSession(
        liveSessionId,
        routePid,
        "Sessione avviata dal bridge Tauri",
        snapshot ? `${Math.round(snapshot.elapsedSecs)}s` : "live",
        derivedStatus,
        snapshot,
        timeline,
      );
      return {
        ...synthetic,
        activePid: snapshot?.activePid ?? routePid,
        lastPid: snapshot?.lastPid ?? routePid,
        title: snapshot?.title ?? `Runtime session / PID ${routePid}`,
      };
    }

    return buildSyntheticSession(
      sessionId,
      snapshot?.activePid ?? snapshot?.lastPid ?? timeline?.pid ?? 0,
      "Sessione persistita dal control plane SQLite",
      snapshot ? `${Math.round(snapshot.elapsedSecs)}s` : "persisted",
      derivedStatus,
      snapshot,
      timeline,
    );
  }, [listedSession, routePid, sessionId, snapshot, timeline?.pid, timeline?.running]);

  const activePid = snapshot?.activePid ?? session?.activePid ?? null;
  const displayedTitle = snapshot?.title ?? session?.title ?? "";
  const displayedRuntimeState = snapshot?.state ?? session?.runtimeState ?? null;
  const pendingHumanRequest = snapshot?.pendingHumanRequest ?? null;

  useEffect(() => {
    if (!sessionId) {
      clear();
      return;
    }

    void refreshTimeline(sessionId, activePid);
    void refresh(sessionId, activePid);
  }, [activePid, clear, refresh, refreshTimeline, sessionId]);

  useEffect(() => {
    setComposerValue("");
    setComposerError(null);
    setComposerLoading(false);
    setTurnActionLoading(false);
    setTurnActionError(null);
    setStopRequestPending(false);
  }, [session?.pid]);

  const awaitingContinuation = snapshot?.state === "AwaitingTurnDecision";
  const canRequestStopWhileRunning =
    !!activePid &&
    !awaitingContinuation &&
    (snapshot?.state === "InFlight" ||
      snapshot?.state === "AwaitingRemoteResponse" ||
      snapshot?.state === "Running");
  const backendSupportsImmediateCancel =
    snapshot?.backendCapabilities?.cancelGeneration ?? false;

  useEffect(() => {
    if (!canRequestStopWhileRunning) {
      setStopRequestPending(false);
    }
  }, [canRequestStopWhileRunning]);

  const stopButtonTitle = stopRequestPending
    ? backendSupportsImmediateCancel
      ? "Interruzione gia' richiesta"
      : "Stop gia' richiesto: il turno si chiudera' alla prossima boundary sicura"
    : backendSupportsImmediateCancel
      ? "Interrompi subito la generazione"
      : "Richiedi stop alla prossima boundary sicura";

  const canResumeFromHistory =
    !activePid &&
    !awaitingContinuation &&
    !loading &&
    !timelineLoading &&
    !composerLoading &&
    !turnActionLoading &&
    Boolean(session && !session.sessionId.startsWith("pid-"));
  const canSendInput =
    canResumeFromHistory ||
    (!!activePid &&
      (snapshot?.state === "WaitingForInput" ||
        snapshot?.state === "WaitingForHumanInput") &&
      !timeline?.running &&
      !composerLoading &&
      !turnActionLoading);
  const canSendTextInput =
    canResumeFromHistory ||
    (canSendInput && (!pendingHumanRequest || pendingHumanRequest.allowFreeText));

  async function refreshWorkspace(sessionKey: string, pid: number) {
    await Promise.all([refreshTimeline(sessionKey, pid), refresh(sessionKey, pid)]);
  }

  async function submitInput(rawPrompt: string) {
    if (!session) {
      return;
    }

    const prompt = rawPrompt.trim();
    if (!prompt) {
      return;
    }

    setComposerLoading(true);
    setComposerError(null);
    setTurnActionError(null);
    setStopRequestPending(false);
    try {
      const result = await sendSessionInput({
        pid: activePid,
        sessionId: session.sessionId,
        prompt,
      });
      setComposerValue("");
      await refreshWorkspace(session.sessionId, result.pid);
    } catch (error) {
      setComposerError(
        error instanceof Error ? error.message : "Failed to resume or send input to session",
      );
    } finally {
      setComposerLoading(false);
    }
  }

  async function handleComposerSubmit() {
    await submitInput(composerValue);
  }

  async function handleHumanChoice(choice: string) {
    await submitInput(choice);
  }

  async function handleContinueOutput() {
    if (!session || !activePid) {
      return;
    }

    setTurnActionLoading(true);
    setTurnActionError(null);
    setComposerError(null);
    setStopRequestPending(false);
    try {
      await continueSessionOutput(activePid);
      await refreshWorkspace(session.sessionId, activePid);
    } catch (error) {
      setTurnActionError(
        error instanceof Error
          ? error.message
          : "Failed to continue truncated assistant output",
      );
    } finally {
      setTurnActionLoading(false);
    }
  }

  async function handleStopOutput() {
    if (!session || !activePid) {
      return;
    }

    setTurnActionLoading(true);
    setTurnActionError(null);
    setComposerError(null);
    try {
      const result = await stopSessionOutput(activePid);
      setStopRequestPending(result.action === "request_stop_output");
      await refreshWorkspace(session.sessionId, activePid);
    } catch (error) {
      setTurnActionError(
        error instanceof Error ? error.message : "Failed to stop assistant output",
      );
    } finally {
      setTurnActionLoading(false);
    }
  }

  if (!session) {
    return (
      <div className="flex items-center justify-center p-20">
        <div className="text-center">
          <h2 className="text-2xl font-bold text-slate-900">Sessione non trovata</h2>
          <p className="mt-3 text-sm text-slate-500 max-w-md mx-auto">
            Questo workspace usa `session_id` come identita' primaria e ricarica i dati da SQLite; la sessione richiesta non e' presente nello store persistito o e' stata cancellata.
          </p>
          <Link
            to="/sessions"
            className="mt-6 inline-flex items-center gap-2 rounded-xl bg-indigo-600 px-6 py-3 text-sm font-semibold text-white shadow-sm hover:bg-indigo-700 transition"
          >
            <ArrowLeft className="w-4 h-4" />
            Torna alle Sessioni
          </Link>
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="max-w-[1600px] w-full mx-auto h-[calc(100vh-4rem)] flex gap-6">
        <div className="flex-1 flex flex-col bg-white border border-slate-200 rounded-2xl shadow-sm overflow-hidden min-w-[500px]">
          <div className="border-b border-slate-100 p-4 shrink-0 flex items-center justify-between">
            <Link
              to="/sessions"
              className="text-slate-400 hover:text-indigo-600 hover:bg-indigo-50 p-2 rounded-xl transition"
              title="Return to Sessions"
            >
              <ArrowLeft className="w-5 h-5" />
            </Link>
            <div className="text-center">
               <h1 className="font-bold text-slate-900">{displayedTitle}</h1>
               <div className="text-xs text-slate-500 uppercase font-semibold tracking-wider">
                 session:{session.status} · runtime:{runtimeStateLabel(displayedRuntimeState)} · PID {activePid ?? session.pid}
               </div>
            </div>
            <div className="w-9" /> {/* spacer for alignment */}
          </div>
          <div className="flex-1 overflow-y-auto">
            <TimelinePane
              timeline={timeline}
              loading={timelineLoading}
              error={timelineError}
              awaitingContinuation={awaitingContinuation}
              canRequestStopWhileRunning={canRequestStopWhileRunning}
              stopButtonTitle={stopButtonTitle}
              stopRequestPending={stopRequestPending}
              composerValue={composerValue}
              composerLoading={composerLoading}
              composerError={composerError}
              turnActionLoading={turnActionLoading}
              turnActionError={turnActionError}
              canSend={canSendInput}
              canSendText={canSendTextInput}
              humanRequest={pendingHumanRequest}
              onComposerChange={setComposerValue}
              onComposerSubmit={handleComposerSubmit}
              onHumanChoice={handleHumanChoice}
              onContinueOutput={handleContinueOutput}
              onStopOutput={handleStopOutput}
            />
          </div>
        </div>

        <div className="bg-white border rounded-2xl border-slate-200 overflow-hidden shadow-sm shrink-0 min-h-0 flex">
          <MindPanel 
            session={session} 
            snapshot={snapshot} 
            loading={loading} 
            error={error} 
            onOpenAudit={() => setAuditOpen(true)}
          />
        </div>
      </div>

      <AuditDrawer 
        isOpen={auditOpen} 
        onClose={() => setAuditOpen(false)} 
        snapshot={snapshot} 
      />
    </>
  );
}
