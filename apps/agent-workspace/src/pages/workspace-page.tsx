import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { MindPanel } from "../components/workspace/mind-panel";
import { TimelinePane } from "../components/workspace/timeline-pane";
import {
  continueSessionOutput,
  sendSessionInput,
  stopSessionOutput,
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";
import { useWorkspaceStore } from "../store/workspace-store";

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

  const routePid =
    sessionId && sessionId.startsWith("pid-") ? Number(sessionId.slice(4)) : Number.NaN;
  const session = useMemo(() => {
    if (!sessionId) {
      return undefined;
    }

    if (listedSession) {
      return listedSession;
    }

    const derivedStatus: "idle" | "running" | "swapped" =
      snapshot?.state === "Parked"
        ? "swapped"
        : timeline?.running ||
            snapshot?.state === "Running" ||
            snapshot?.state === "WaitingForSyscall" ||
            snapshot?.state === "InFlight"
          ? "running"
          : "idle";

    if (!Number.isNaN(routePid)) {
      return {
        sessionId: sessionId ?? `pid-${routePid}`,
        pid: routePid,
        activePid: snapshot?.activePid ?? routePid,
        lastPid: snapshot?.lastPid ?? routePid,
        title: snapshot?.title ?? `Runtime session / PID ${routePid}`,
        promptPreview: "Sessione avviata dal bridge Tauri",
        status: derivedStatus,
        uptimeLabel: snapshot ? `${Math.round(snapshot.elapsedSecs)}s` : "live",
        tokensLabel: snapshot ? String(snapshot.tokensGenerated) : "0",
        contextStrategy: snapshot?.context?.contextStrategy ?? "sliding_window",
        runtimeId: snapshot?.runtimeId ?? null,
        runtimeLabel: snapshot?.runtimeLabel ?? null,
        backendClass: snapshot?.backendClass ?? null,
      };
    }

    return {
      sessionId,
      pid: snapshot?.activePid ?? snapshot?.lastPid ?? timeline?.pid ?? 0,
      activePid: snapshot?.activePid ?? null,
      lastPid: snapshot?.lastPid ?? (timeline?.pid ?? null),
      title: snapshot?.title ?? `Session ${sessionId}`,
      promptPreview: "Sessione persistita dal control plane SQLite",
      status: derivedStatus,
      uptimeLabel: snapshot ? `${Math.round(snapshot.elapsedSecs)}s` : "persisted",
      tokensLabel: snapshot ? String(snapshot.tokensGenerated) : "0",
      contextStrategy: snapshot?.context?.contextStrategy ?? "sliding_window",
      runtimeId: snapshot?.runtimeId ?? null,
      runtimeLabel: snapshot?.runtimeLabel ?? null,
      backendClass: snapshot?.backendClass ?? null,
    };
  }, [listedSession, routePid, sessionId, snapshot, timeline?.pid, timeline?.running]);

  const activePid = snapshot?.activePid ?? session?.activePid ?? null;

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
  }, [session?.pid]);

  const awaitingContinuation = snapshot?.state === "AwaitingTurnDecision";
  const canSendInput =
    !!activePid &&
    snapshot?.state === "WaitingForInput" &&
    !timeline?.running &&
    !composerLoading &&
    !turnActionLoading;

  async function handleComposerSubmit() {
    if (!session || !activePid) {
      return;
    }

    const prompt = composerValue.trim();
    if (!prompt) {
      return;
    }

    setComposerLoading(true);
    setComposerError(null);
    setTurnActionError(null);
    try {
      await sendSessionInput(activePid, prompt);
      setComposerValue("");
      await Promise.all([
        refreshTimeline(session.sessionId, activePid),
        refresh(session.sessionId, activePid),
      ]);
    } catch (error) {
      setComposerError(
        error instanceof Error ? error.message : "Failed to send input to resident PID",
      );
    } finally {
      setComposerLoading(false);
    }
  }

  async function handleContinueOutput() {
    if (!session || !activePid) {
      return;
    }

    setTurnActionLoading(true);
    setTurnActionError(null);
    setComposerError(null);
    try {
      await continueSessionOutput(activePid);
      await Promise.all([
        refreshTimeline(session.sessionId, activePid),
        refresh(session.sessionId, activePid),
      ]);
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
    try {
      await stopSessionOutput(activePid);
      await Promise.all([
        refreshTimeline(session.sessionId, activePid),
        refresh(session.sessionId, activePid),
      ]);
    } catch (error) {
      setTurnActionError(
        error instanceof Error ? error.message : "Failed to stop truncated assistant output",
      );
    } finally {
      setTurnActionLoading(false);
    }
  }

  if (!session) {
    return (
      <div className="panel-surface px-6 py-10 text-center">
        <h2 className="text-2xl font-bold text-slate-950">Sessione non trovata</h2>
        <p className="mt-3 text-sm text-slate-600">
          Questo workspace usa `session_id` come identita' primaria e ricarica i dati da SQLite; la sessione richiesta non e' presente nello store persistito.
        </p>
        <Link
          to="/"
          className="mt-5 inline-flex rounded-full bg-slate-950 px-5 py-2.5 text-sm font-semibold text-white"
        >
          Torna alla Lobby
        </Link>
      </div>
    );
  }

  return (
    <section className="space-y-4">
      <div className="flex justify-end">
        <Link
          to="/"
          className="rounded-full border border-slate-900/10 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:border-slate-900/20 hover:text-slate-950"
        >
          Torna alla Lobby
        </Link>
      </div>
      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.9fr)_minmax(320px,0.9fr)]">
        <TimelinePane
          session={session}
          timeline={timeline}
          loading={timelineLoading}
          error={timelineError}
          awaitingContinuation={awaitingContinuation}
          composerValue={composerValue}
          composerLoading={composerLoading}
          composerError={composerError}
          turnActionLoading={turnActionLoading}
          turnActionError={turnActionError}
          canSend={canSendInput}
          onComposerChange={setComposerValue}
          onComposerSubmit={handleComposerSubmit}
          onContinueOutput={handleContinueOutput}
          onStopOutput={handleStopOutput}
        />
        <MindPanel session={session} snapshot={snapshot} loading={loading} error={error} />
      </div>
    </section>
  );
}
