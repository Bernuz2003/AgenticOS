import { invoke } from "@tauri-apps/api/core";

import type {
  LobbySnapshot,
  LobbySnapshotDto,
  TimelineSnapshot,
  TimelineSnapshotDto,
  WorkspaceSnapshot,
  WorkspaceSnapshotDto,
} from "./index";
import {
  normalizeLobbySnapshot,
  normalizeTimelineSnapshot,
  normalizeWorkspaceSnapshot,
} from "./normalizers";

export async function fetchLobbySnapshot(): Promise<LobbySnapshot> {
  const snapshot = await invoke<LobbySnapshotDto>("fetch_lobby_snapshot");
  return normalizeLobbySnapshot(snapshot);
}

export async function fetchWorkspaceSnapshot(
  sessionId: string,
  pid: number | null,
): Promise<WorkspaceSnapshot> {
  const snapshot = await invoke<WorkspaceSnapshotDto>("fetch_workspace_snapshot", {
    sessionId,
    pid,
  });
  return normalizeWorkspaceSnapshot(snapshot);
}

export async function fetchTimelineSnapshot(
  sessionId: string,
  pid: number | null,
): Promise<TimelineSnapshot> {
  const snapshot = await invoke<TimelineSnapshotDto>("fetch_timeline_snapshot", {
    sessionId,
    pid,
  });
  return normalizeTimelineSnapshot(snapshot);
}
