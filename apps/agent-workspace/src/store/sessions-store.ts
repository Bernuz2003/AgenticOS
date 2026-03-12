import { create } from "zustand";

import {
  fetchLobbySnapshot,
  type BackendCapabilities,
  type BackendTelemetry,
  type RemoteRuntimeModel,
} from "../lib/api";

export type SessionStatus = "idle" | "running" | "swapped";

export interface AgentSessionSummary {
  sessionId: string;
  pid: number;
  title: string;
  promptPreview: string;
  status: SessionStatus;
  uptimeLabel: string;
  tokensLabel: string;
  contextStrategy: string;
  orchestrationId?: number | null;
  orchestrationTaskId?: string | null;
}

export interface LobbyOrchestrationSummary {
  orchestrationId: number;
  total: number;
  completed: number;
  running: number;
  pending: number;
  failed: number;
  skipped: number;
  finished: boolean;
  elapsedLabel: string;
  policy: string;
}

interface SessionsState {
  sessions: AgentSessionSummary[];
  orchestrations: LobbyOrchestrationSummary[];
  connected: boolean;
  selectedModelId: string;
  loadedModelId: string;
  loadedTargetKind: string | null;
  loadedProviderId: string | null;
  loadedRemoteModelId: string | null;
  loadedBackendId: string | null;
  loadedBackendClass: string | null;
  loadedBackendCapabilities: BackendCapabilities | null;
  loadedBackendTelemetry: BackendTelemetry | null;
  loadedRemoteModel: RemoteRuntimeModel | null;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  applySnapshot: (snapshot: Awaited<ReturnType<typeof fetchLobbySnapshot>>) => void;
  setBridgeStatus: (connected: boolean, error: string | null) => void;
}

function normalizeStatus(status: string): SessionStatus {
  switch (status) {
    case "running":
      return "running";
    case "swapped":
      return "swapped";
    default:
      return "idle";
  }
}

export const useSessionsStore = create<SessionsState>((set) => ({
  sessions: [],
  orchestrations: [],
  connected: false,
  selectedModelId: "",
  loadedModelId: "",
  loadedTargetKind: null,
  loadedProviderId: null,
  loadedRemoteModelId: null,
  loadedBackendId: null,
  loadedBackendClass: null,
  loadedBackendCapabilities: null,
  loadedBackendTelemetry: null,
  loadedRemoteModel: null,
  loading: false,
  error: null,
  applySnapshot: (snapshot) => {
    set({
      connected: snapshot.connected,
      selectedModelId: snapshot.selectedModelId,
      loadedModelId: snapshot.loadedModelId,
      loadedTargetKind: snapshot.loadedTargetKind,
      loadedProviderId: snapshot.loadedProviderId,
      loadedRemoteModelId: snapshot.loadedRemoteModelId,
      loadedBackendId: snapshot.loadedBackendId,
      loadedBackendClass: snapshot.loadedBackendClass,
      loadedBackendCapabilities: snapshot.loadedBackendCapabilities,
      loadedBackendTelemetry: snapshot.loadedBackendTelemetry,
      loadedRemoteModel: snapshot.loadedRemoteModel,
      orchestrations: snapshot.orchestrations,
      error: snapshot.error,
      loading: false,
      sessions: snapshot.sessions.map((session) => ({
        sessionId: session.sessionId,
        pid: session.pid,
        title: session.title,
        promptPreview: session.promptPreview,
        status: normalizeStatus(session.status),
        uptimeLabel: session.uptimeLabel,
        tokensLabel: session.tokensLabel,
        contextStrategy: session.contextStrategy || "sliding_window",
        orchestrationId: session.orchestrationId,
        orchestrationTaskId: session.orchestrationTaskId,
      })),
    });
  },
  setBridgeStatus: (connected, error) => {
    set((state) => ({
      connected,
      error,
      loading: false,
      sessions: connected ? state.sessions : state.sessions,
      orchestrations: connected ? state.orchestrations : state.orchestrations,
    }));
  },
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const snapshot = await fetchLobbySnapshot();
      useSessionsStore.getState().applySnapshot(snapshot);
    } catch (error) {
      set({
        connected: false,
        loading: false,
        selectedModelId: "",
        loadedModelId: "",
        loadedTargetKind: null,
        loadedProviderId: null,
        loadedRemoteModelId: null,
        loadedBackendId: null,
        loadedBackendClass: null,
        loadedBackendCapabilities: null,
        loadedBackendTelemetry: null,
        loadedRemoteModel: null,
        orchestrations: [],
        sessions: [],
        error: error instanceof Error ? error.message : "Failed to fetch lobby snapshot",
      });
    }
  },
}));
