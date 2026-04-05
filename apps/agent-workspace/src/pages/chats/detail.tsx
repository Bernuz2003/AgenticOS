import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { ArrowLeft } from "lucide-react";

import { AuditDrawer } from "../../components/diagnostics/event-log";
import { ConversationSurface } from "../../components/workspace/conversation-surface";
import { DebugMode } from "../../components/workspace/debug-mode";
import { InspectDrawer } from "../../components/workspace/inspect-drawer";
import { SessionShell } from "../../components/workspace/session-shell";
import { SessionHeader } from "../../components/workspace/session-shell/session-header";
import { useCoreDumps } from "../../hooks/useCoreDumps";
import {
  continueSessionOutput,
  sendSessionInput,
  stopSessionOutput,
  type ReplayCoreDumpResult,
} from "../../lib/api";
import {
  deriveSessionStatus,
  isRuntimeActiveState,
} from "../../lib/utils/formatting";
import { updateWorkspaceSearchParams, workspaceModeFromSearch } from "../../lib/workspace/view-state";
import { useSessionsStore } from "../../store/sessions-store";
import type { AgentSessionSummary } from "../../store/sessions-store";
import { useWorkspaceStore } from "../../store/workspace-store";

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
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const listedSession = useSessionsStore((state) =>
    state.sessions.find((item) => item.sessionId === sessionId),
  );
  const snapshot = useWorkspaceStore((state) => state.snapshot);
  const timeline = useWorkspaceStore((state) => state.timeline);
  const loading = useWorkspaceStore((state) => state.loading);
  const liveSnapshotRevision = useWorkspaceStore(
    (state) => state.liveSnapshotRevision,
  );
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
  const [compactionToast, setCompactionToast] = useState<string | null>(null);
  const lastCompactionRef = useRef<string | null>(null);
  const timelineResyncRef = useRef<string | null>(null);

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
  const pendingHumanRequest = snapshot?.pendingHumanRequest ?? null;
  const timelineHasStreamingItems = useMemo(
    () => timeline?.items.some((item) => item.status === "streaming") ?? false,
    [timeline?.items],
  );
  const mode = workspaceModeFromSearch(
    searchParams.get("mode"),
    searchParams.get("dump"),
  );
  const selectedDumpId = searchParams.get("dump");
  const coreDumps = useCoreDumps({
    sessionId: session?.sessionId ?? sessionId ?? "",
    pid: activePid ?? session?.lastPid ?? null,
    refreshKey: liveSnapshotRevision,
    selectedDumpId,
  });

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

  useEffect(() => {
    const currentReason = snapshot?.context?.lastCompactionReason ?? null;
    if (currentReason && currentReason !== lastCompactionRef.current) {
      setCompactionToast(currentReason);
      const timeout = window.setTimeout(() => setCompactionToast(null), 4000);
      lastCompactionRef.current = currentReason;
      return () => window.clearTimeout(timeout);
    }

    lastCompactionRef.current = currentReason;
    return undefined;
  }, [snapshot?.context?.lastCompactionReason]);

  useEffect(() => {
    if (!sessionId || !snapshot || !timelineHasStreamingItems) {
      timelineResyncRef.current = null;
      return;
    }

    if (isRuntimeActiveState(snapshot.state)) {
      timelineResyncRef.current = null;
      return;
    }

    const refreshPid = activePid ?? snapshot.activePid ?? snapshot.pid;
    const resyncKey = `${sessionId}:${refreshPid}:${snapshot.state}:${timelineHasStreamingItems}`;
    if (timelineResyncRef.current === resyncKey) {
      return;
    }

    timelineResyncRef.current = resyncKey;
    void refreshTimeline(sessionId, refreshPid);
  }, [
    activePid,
    refreshTimeline,
    sessionId,
    snapshot,
    timelineHasStreamingItems,
  ]);

  useEffect(() => {
    if (mode !== "debug") {
      return;
    }
    if (selectedDumpId === coreDumps.resolvedSelectedDumpId) {
      return;
    }
    setSearchParams(
      (current) =>
        updateWorkspaceSearchParams(current, {
          dump: coreDumps.resolvedSelectedDumpId,
        }),
      { replace: true },
    );
  }, [coreDumps.resolvedSelectedDumpId, mode, selectedDumpId, setSearchParams]);

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

  function applyViewState(patch: {
    mode?: "conversation" | "debug" | null;
    dump?: string | null;
  }) {
    setSearchParams(
      (current) => updateWorkspaceSearchParams(current, patch),
      { replace: true },
    );
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
    } catch (nextError) {
      setComposerError(
        nextError instanceof Error
          ? nextError.message
          : "Failed to resume or send input to session",
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
    } catch (nextError) {
      setTurnActionError(
        nextError instanceof Error
          ? nextError.message
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
    } catch (nextError) {
      setTurnActionError(
        nextError instanceof Error ? nextError.message : "Failed to stop assistant output",
      );
    } finally {
      setTurnActionLoading(false);
    }
  }

  function handleReplayReady(result: ReplayCoreDumpResult) {
    navigate(`/workspace/${result.sessionId}`);
  }

  async function handleCaptureDump() {
    const dump = await coreDumps.captureDump();
    if (dump) {
      applyViewState({ mode: "debug", dump: dump.dumpId });
    }
  }

  async function handleReplayDump(dumpId: string, branchLabel?: string | null) {
    const result = await coreDumps.replayDump(dumpId, branchLabel);
    if (result) {
      handleReplayReady(result);
    }
  }

  function handleSelectDump(dumpId: string) {
    applyViewState({ mode: "debug", dump: dumpId });
    void coreDumps.selectDump(dumpId);
  }

  function handleOpenAudit() {
    setAuditOpen(true);
  }

  function handleToggleWorkspaceMode() {
    if (mode === "debug") {
      applyViewState({ mode: "conversation", dump: null });
      return;
    }
    applyViewState({ mode: "debug", dump: coreDumps.resolvedSelectedDumpId });
  }

  if (!session) {
    return (
      <div className="flex items-center justify-center p-20">
        <div className="text-center">
          <h2 className="text-2xl font-bold text-slate-900">Sessione non trovata</h2>
          <p className="mx-auto mt-3 max-w-md text-sm text-slate-500">
            Questo workspace usa `session_id` come identita' primaria e ricarica i dati da
            SQLite; la sessione richiesta non e' presente nello store persistito o e' stata
            cancellata.
          </p>
          <Link
            to="/sessions"
            className="mt-6 inline-flex items-center gap-2 rounded-xl bg-indigo-600 px-6 py-3 text-sm font-semibold text-white shadow-sm transition hover:bg-indigo-700"
          >
            <ArrowLeft className="h-4 w-4" />
            Torna alle Sessioni
          </Link>
        </div>
      </div>
    );
  }

  return (
    <>
      <SessionShell
        header={
          <SessionHeader
            session={session}
            snapshot={snapshot}
            mode={mode}
            onOpenAudit={handleOpenAudit}
            onToggleWorkspaceMode={handleToggleWorkspaceMode}
          />
        }
      >
        {mode === "debug" ? (
          <DebugMode
            dumps={coreDumps.dumps}
            selectedDumpId={coreDumps.resolvedSelectedDumpId}
            selectedInfo={coreDumps.selectedInfo}
            loading={coreDumps.loading}
            capturePending={coreDumps.capturePending}
            replayPendingId={coreDumps.replayPendingId}
            activeReplaySourceDumpId={snapshot?.replay?.sourceDumpId ?? null}
            currentReplay={snapshot?.replay ?? null}
            canCapture={activePid !== null}
            onRefresh={() => void coreDumps.refreshDumps()}
            onCapture={() => void handleCaptureDump()}
            onSelectDump={handleSelectDump}
            onReplayDump={handleReplayDump}
          />
        ) : (
          <div className="mx-auto flex h-full w-full max-w-[1480px] min-h-0 justify-center gap-4">
            <div className="flex min-h-0 min-w-0 max-w-[980px] flex-1">
              <ConversationSurface
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
            <InspectDrawer
              session={session}
              snapshot={snapshot}
              compactionToast={compactionToast}
            />
          </div>
        )}
      </SessionShell>

      {error ? (
        <div className="fixed bottom-6 left-6 z-30 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800 shadow-lg">
          {error}
        </div>
      ) : null}

      {compactionToast ? (
        <div className="fixed bottom-6 right-6 z-30 max-w-md rounded-[24px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-950 shadow-lg">
          <div className="font-semibold">Context compaction</div>
          <div className="mt-1 leading-relaxed">{compactionToast}</div>
        </div>
      ) : null}

      <AuditDrawer isOpen={auditOpen} onClose={() => setAuditOpen(false)} snapshot={snapshot} />
    </>
  );
}
