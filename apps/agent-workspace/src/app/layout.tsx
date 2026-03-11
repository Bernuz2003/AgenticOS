import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Orbit, Signal } from "lucide-react";
import { Link, Outlet, useLocation, useNavigate } from "react-router-dom";
import type { LobbySnapshot, TimelineSnapshot, WorkspaceSnapshot } from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";
import { useWorkspaceStore } from "../store/workspace-store";

function focusNewAgentComposer() {
  const card = document.getElementById("new-agent-card");
  const prompt = document.getElementById("new-agent-prompt") as HTMLTextAreaElement | null;

  card?.scrollIntoView({ behavior: "smooth", block: "center" });
  prompt?.focus();
}

export function AppLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const connected = useSessionsStore((state) => state.connected);
  const loading = useSessionsStore((state) => state.loading);
  const error = useSessionsStore((state) => state.error);
  const refresh = useSessionsStore((state) => state.refresh);
  const applyLobbySnapshot = useSessionsStore((state) => state.applySnapshot);
  const setBridgeStatus = useSessionsStore((state) => state.setBridgeStatus);
  const applyWorkspaceSnapshot = useWorkspaceStore((state) => state.applySnapshot);
  const applyTimeline = useWorkspaceStore((state) => state.applyTimeline);
  const inWorkspace = location.pathname.startsWith("/workspace/");

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    const cleanup: UnlistenFn[] = [];

    const register = async () => {
      const handlers = await Promise.all([
        listen<LobbySnapshot>("kernel://lobby_snapshot", (event) => {
          applyLobbySnapshot(event.payload);
        }),
        listen<WorkspaceSnapshot>("kernel://workspace_snapshot", (event) => {
          applyWorkspaceSnapshot(event.payload);
        }),
        listen<TimelineSnapshot>("kernel://timeline_snapshot", (event) => {
          applyTimeline(event.payload);
        }),
        listen<{ connected: boolean; error: string | null }>(
          "kernel://bridge_status",
          (event) => {
            setBridgeStatus(event.payload.connected, event.payload.error);
          },
        ),
      ]);

      if (cancelled) {
        handlers.forEach((unlisten) => unlisten());
        return;
      }

      cleanup.push(...handlers);
    };

    void register();

    return () => {
      cancelled = true;
      cleanup.forEach((unlisten) => unlisten());
    };
  }, [applyLobbySnapshot, applyTimeline, applyWorkspaceSnapshot, setBridgeStatus]);

  function handleComposeClick() {
    if (inWorkspace) {
      navigate("/", { state: { focusComposer: true } });
      return;
    }

    focusNewAgentComposer();
  }

  const badgeTone = connected
    ? "border-emerald-600/20 bg-emerald-50 text-emerald-700"
    : loading
      ? "border-amber-500/20 bg-amber-50 text-amber-700"
      : "border-rose-500/20 bg-rose-50 text-rose-700";
  const badgeLabel = connected
    ? "Bridge connected"
    : loading
      ? "Bridge syncing"
      : "Bridge disconnected";

  return (
    <div className="min-h-screen px-5 py-6 text-slate-900 sm:px-8 lg:px-10">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <header className="panel-surface flex flex-col gap-4 px-6 py-5 md:flex-row md:items-center md:justify-between">
          <div className="space-y-2">
            <div className="flex items-center gap-3 text-[11px] font-semibold uppercase tracking-[0.34em] text-slate-500">
              <Orbit className="h-4 w-4" />
              Agent Workspace
            </div>
            <div>
              <h1 className="text-3xl font-bold tracking-tight text-slate-950">
                Workflow Inspector
              </h1>
              <p className="max-w-2xl text-sm text-slate-600">
                Lobby a sessioni e workspace telemetrico per osservare il comportamento cognitivo dell&apos;agente sopra il kernel TCP locale.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <span className={`status-pill ${badgeTone}`}>
              <Signal className="h-3.5 w-3.5" />
              {badgeLabel}
            </span>
            {inWorkspace ? (
              <Link
                to="/"
                className="rounded-full border border-slate-900/10 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:border-slate-900/20 hover:text-slate-950"
              >
                Torna alla Lobby
              </Link>
            ) : (
              <button
                onClick={handleComposeClick}
                className="rounded-full bg-slate-950 px-5 py-2.5 text-sm font-semibold text-white transition hover:bg-slate-800"
              >
                Nuovo Agente / Nuova Chat
              </button>
            )}
          </div>
          {!connected && error ? (
            <div className="text-xs text-rose-700">{error}</div>
          ) : null}
        </header>

        <main>
          <Outlet />
        </main>

        <footer className="px-2 text-xs text-slate-500">
          Tauri shell, React workspace UI, Rust bridge TCP autenticato verso AgenticOS.
        </footer>
      </div>
    </div>
  );
}
