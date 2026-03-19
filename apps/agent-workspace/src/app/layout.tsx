import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Outlet } from "react-router-dom";
import {
  type AuditEventDto,
  type LobbySnapshotDto,
  type TimelineSnapshotDto,
  type WorkspaceSnapshotDto,
  normalizeAuditEvent,
  normalizeLobbySnapshot,
  normalizeTimelineSnapshot,
  normalizeWorkspaceSnapshot,
} from "../lib/api";
import { useSessionsStore } from "../store/sessions-store";
import { useWorkspaceStore } from "../store/workspace-store";
import { Sidebar } from "../components/layout/sidebar";

export function AppLayout() {
  const refresh = useSessionsStore((state) => state.refresh);
  const applyLobbySnapshot = useSessionsStore((state) => state.applySnapshot);
  const appendGlobalAuditEvent = useSessionsStore(
    (state) => state.appendGlobalAuditEvent,
  );
  const setBridgeStatus = useSessionsStore((state) => state.setBridgeStatus);
  const applyWorkspaceSnapshot = useWorkspaceStore((state) => state.applyLiveSnapshot);
  const applyTimeline = useWorkspaceStore((state) => state.applyLiveTimeline);
  const appendWorkspaceAuditEvent = useWorkspaceStore(
    (state) => state.appendLiveAuditEvent,
  );

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
        listen<AuditEventDto>("kernel://diagnostic_event", (event) => {
          const auditEvent = normalizeAuditEvent(event.payload);
          appendGlobalAuditEvent(auditEvent);
          appendWorkspaceAuditEvent(auditEvent);
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
  }, [
    appendGlobalAuditEvent,
    appendWorkspaceAuditEvent,
    applyLobbySnapshot,
    applyTimeline,
    applyWorkspaceSnapshot,
    setBridgeStatus,
  ]);

  return (
    <div className="min-h-screen bg-white text-slate-900 flex">
      <Sidebar />
      <div className="flex-1 ml-64 flex flex-col min-h-screen">
        <main className="flex-1 p-6 md:p-8 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
