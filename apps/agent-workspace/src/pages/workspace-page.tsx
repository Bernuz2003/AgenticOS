import { useEffect, useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import { MindPanel } from "../components/workspace/mind-panel";
import { TimelinePane } from "../components/workspace/timeline-pane";
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

  const routePid =
    sessionId && sessionId.startsWith("pid-") ? Number(sessionId.slice(4)) : Number.NaN;
  const session = useMemo(() => {
    if (listedSession) {
      return listedSession;
    }

    if (Number.isNaN(routePid)) {
      return undefined;
    }

    const derivedStatus: "idle" | "running" | "swapped" =
      snapshot?.state === "WaitingForMemory"
        ? "swapped"
        : timeline?.running ||
            snapshot?.state === "Running" ||
            snapshot?.state === "WaitingForSyscall" ||
            snapshot?.state === "InFlight"
          ? "running"
          : "idle";

    return {
      sessionId: sessionId ?? `pid-${routePid}`,
      pid: routePid,
      title: `Runtime session / PID ${routePid}`,
      promptPreview: "Sessione avviata dal bridge Tauri",
      status: derivedStatus,
      uptimeLabel: snapshot ? `${Math.round(snapshot.elapsedSecs)}s` : "live",
      tokensLabel: snapshot ? String(snapshot.tokensGenerated) : "0",
      contextStrategy: snapshot?.context?.contextStrategy ?? "sliding_window",
    };
  }, [listedSession, routePid, sessionId, snapshot, timeline?.running]);

  useEffect(() => {
    if (!session?.pid) {
      clear();
      return;
    }

    const pid = session.pid;
    void refresh(pid);
    void refreshTimeline(pid);
  }, [clear, refresh, refreshTimeline, session?.pid]);

  if (!session) {
    return (
      <div className="panel-surface px-6 py-10 text-center">
        <h2 className="text-2xl font-bold text-slate-950">Sessione non trovata</h2>
        <p className="mt-3 text-sm text-slate-600">
          La Lobby usa ora session state reale da `STATUS`; questo workspace mostra solo PID/sessioni presenti nell'ultimo snapshot disponibile.
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
    <section className="grid gap-5 xl:grid-cols-[minmax(0,1.9fr)_minmax(320px,0.9fr)]">
      <TimelinePane
        session={session}
        timeline={timeline}
        loading={timelineLoading}
        error={timelineError}
      />
      <MindPanel session={session} snapshot={snapshot} loading={loading} error={error} />
    </section>
  );
}
