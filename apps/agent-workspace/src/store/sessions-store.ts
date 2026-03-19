import { create } from "zustand";

import {
  auditEventKey,
  fetchLobbySnapshot,
  type AuditEvent,
  type BackendCapabilities,
  type BackendTelemetry,
  type MemoryStatus,
  type RemoteRuntimeModel,
  type ResourceGovernorStatus,
  type RuntimeInstance,
  type RuntimeLoadQueueEntry,
} from "../lib/api";

export type SessionStatus = "idle" | "running" | "swapped";

export interface AgentSessionSummary {
  sessionId: string;
  pid: number;
  activePid: number | null;
  lastPid: number | null;
  title: string;
  promptPreview: string;
  status: SessionStatus;
  runtimeState: string | null;
  uptimeLabel: string;
  tokensLabel: string;
  contextStrategy: string;
  runtimeId: string | null;
  runtimeLabel: string | null;
  backendClass: string | null;
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
  globalAccounting: BackendTelemetry | null;
  loadedBackendTelemetry: BackendTelemetry | null;
  loadedRemoteModel: RemoteRuntimeModel | null;
  memory: MemoryStatus | null;
  runtimeInstances: RuntimeInstance[];
  resourceGovernor: ResourceGovernorStatus | null;
  runtimeLoadQueue: RuntimeLoadQueueEntry[];
  globalAuditEvents: AuditEvent[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  applySnapshot: (snapshot: Awaited<ReturnType<typeof fetchLobbySnapshot>>) => void;
  appendGlobalAuditEvent: (event: AuditEvent) => void;
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
  globalAccounting: null,
  loadedBackendTelemetry: null,
  loadedRemoteModel: null,
  memory: null,
  runtimeInstances: [],
  resourceGovernor: null,
  runtimeLoadQueue: [],
  globalAuditEvents: [],
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
      globalAccounting: snapshot.globalAccounting,
      loadedBackendTelemetry: snapshot.loadedBackendTelemetry,
      loadedRemoteModel: snapshot.loadedRemoteModel,
      memory: snapshot.memory,
      runtimeInstances: snapshot.runtimeInstances,
      resourceGovernor: snapshot.resourceGovernor,
      runtimeLoadQueue: snapshot.runtimeLoadQueue,
      globalAuditEvents: snapshot.globalAuditEvents,
      orchestrations: snapshot.orchestrations,
      error: snapshot.error,
      loading: false,
      sessions: snapshot.sessions.map((session) => ({
        sessionId: session.sessionId,
        pid: session.pid,
        activePid: session.activePid,
        lastPid: session.lastPid,
        title: session.title,
        promptPreview: session.promptPreview,
        status: normalizeStatus(session.status),
        runtimeState: session.runtimeState,
        uptimeLabel: session.uptimeLabel,
        tokensLabel: session.tokensLabel,
        contextStrategy: session.contextStrategy || "sliding_window",
        runtimeId: session.runtimeId,
        runtimeLabel: session.runtimeLabel,
        backendClass: session.backendClass,
        orchestrationId: session.orchestrationId,
        orchestrationTaskId: session.orchestrationTaskId,
      })),
    });
  },
  appendGlobalAuditEvent: (event) => {
    set((state) => {
      const eventKey = auditEventKey(event);
      if (
        state.globalAuditEvents.some(
          (candidate) => auditEventKey(candidate) === eventKey,
        )
      ) {
        return state;
      }

      return {
        globalAuditEvents: [event, ...state.globalAuditEvents]
          .sort((left, right) => right.recordedAtMs - left.recordedAtMs)
          .slice(0, 128),
      };
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
        globalAccounting: null,
        loadedBackendTelemetry: null,
        loadedRemoteModel: null,
        memory: null,
        runtimeInstances: [],
        resourceGovernor: null,
        runtimeLoadQueue: [],
        globalAuditEvents: [],
        orchestrations: [],
        sessions: [],
        error: error instanceof Error ? error.message : "Failed to fetch lobby snapshot",
      });
    }
  },
}));
