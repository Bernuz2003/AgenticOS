import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Outlet } from "react-router-dom";
import {
  type LobbySnapshotDto,
  type TimelineSnapshotDto,
  type WorkspaceSnapshotDto,
  normalizeLobbySnapshot,
  normalizeTimelineSnapshot,
  normalizeWorkspaceSnapshot,
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";
import { useWorkspaceStore } from "../store/workspace-store";

export function AppLayout() {
  const refresh = useSessionsStore((state) => state.refresh);
  const applyLobbySnapshot = useSessionsStore((state) => state.applySnapshot);
  const setBridgeStatus = useSessionsStore((state) => state.setBridgeStatus);
  const applyWorkspaceSnapshot = useWorkspaceStore((state) => state.applySnapshot);
  const applyTimeline = useWorkspaceStore((state) => state.applyTimeline);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    const cleanup: UnlistenFn[] = [];

    const register = async () => {
      const handlers = await Promise.all([
        listen<LobbySnapshotDto>("kernel://lobby_snapshot", (event) => {
          applyLobbySnapshot(normalizeLobbySnapshot(event.payload));
        }),
        listen<WorkspaceSnapshotDto>("kernel://workspace_snapshot", (event) => {
          applyWorkspaceSnapshot(normalizeWorkspaceSnapshot(event.payload));
        }),
        listen<TimelineSnapshotDto>("kernel://timeline_snapshot", (event) => {
          applyTimeline(normalizeTimelineSnapshot(event.payload));
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

  return (
    <div className="min-h-screen px-5 py-6 text-slate-900 sm:px-8 lg:px-10">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
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
