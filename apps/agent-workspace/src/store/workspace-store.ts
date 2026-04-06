import { create } from "zustand";

import {
  auditEventKey,
  fetchTimelineSnapshot,
  fetchWorkspaceSnapshot,
  type AuditEvent,
  type TimelineSnapshot,
  type WorkspaceSnapshot,
} from "../lib/api";

let snapshotRequestSeq = 0;
let timelineRequestSeq = 0;

interface WorkspaceMatchCandidate {
  sessionId: string;
  pid: number | null;
}

function isPidFallbackSessionId(sessionId: string | null): boolean {
  return sessionId !== null && sessionId.startsWith("pid-");
}

interface WorkspaceState {
  activeSessionId: string | null;
  activePid: number | null;
  snapshot: WorkspaceSnapshot | null;
  timeline: TimelineSnapshot | null;
  loading: boolean;
  timelineLoading: boolean;
  error: string | null;
  timelineError: string | null;
  liveSnapshotRevision: number;
  liveTimelineRevision: number;
  refresh: (sessionId: string, pid: number | null) => Promise<void>;
  refreshTimeline: (sessionId: string, pid: number | null) => Promise<void>;
  applyLiveSnapshot: (snapshot: WorkspaceSnapshot) => void;
  applyLiveTimeline: (timeline: TimelineSnapshot) => void;
  appendLiveAuditEvent: (event: AuditEvent) => void;
  clear: () => void;
}

function collectActiveWorkspacePids(state: Pick<
  WorkspaceState,
  "activePid" | "snapshot" | "timeline"
>): Set<number> {
  const pids = new Set<number>();
  const candidates = [
    state.activePid,
    state.snapshot?.pid,
    state.snapshot?.activePid,
    state.snapshot?.lastPid,
    state.timeline?.pid,
  ];

  for (const candidate of candidates) {
    if (candidate !== null && candidate !== undefined) {
      pids.add(candidate);
    }
  }

  return pids;
}

function matchesActiveWorkspace(
  state: Pick<WorkspaceState, "activeSessionId" | "activePid" | "snapshot" | "timeline">,
  candidate: WorkspaceMatchCandidate,
): boolean {
  if (state.activeSessionId === null && state.activePid === null) {
    return false;
  }

  if (state.activeSessionId !== null && state.activeSessionId === candidate.sessionId) {
    return true;
  }

  return (
    isPidFallbackSessionId(state.activeSessionId) &&
    candidate.pid !== null &&
    collectActiveWorkspacePids(state).has(candidate.pid)
  );
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  activeSessionId: null,
  activePid: null,
  snapshot: null,
  timeline: null,
  loading: false,
  timelineLoading: false,
  error: null,
  timelineError: null,
  liveSnapshotRevision: 0,
  liveTimelineRevision: 0,
  applyLiveSnapshot: (snapshot) =>
    set((state) => {
      if (
        !matchesActiveWorkspace(state, {
          sessionId: snapshot.sessionId,
          pid: snapshot.pid,
        })
      ) {
        return state;
      }

      return {
        activeSessionId: snapshot.sessionId,
        activePid: snapshot.activePid ?? snapshot.pid,
        snapshot,
        loading: false,
        error: null,
        liveSnapshotRevision: state.liveSnapshotRevision + 1,
      };
    }),
  applyLiveTimeline: (timeline) =>
    set((state) => {
      if (
        !matchesActiveWorkspace(state, {
          sessionId: timeline.sessionId,
          pid: timeline.pid,
        })
      ) {
        return state;
      }

      return {
        activeSessionId: timeline.sessionId,
        activePid: timeline.pid,
        timeline,
        timelineLoading: false,
        timelineError: null,
        liveTimelineRevision: state.liveTimelineRevision + 1,
      };
    }),
  appendLiveAuditEvent: (event) =>
    set((state) => {
      const snapshot = state.snapshot;
      if (
        !snapshot ||
        !matchesActiveWorkspace(state, {
          sessionId: event.sessionId ?? snapshot.sessionId,
          pid: event.pid,
        })
      ) {
        return state;
      }

      const eventKey = auditEventKey(event);
      if (snapshot.auditEvents.some((candidate) => auditEventKey(candidate) === eventKey)) {
        return state;
      }

      const auditEvents = [event, ...snapshot.auditEvents]
        .sort((left, right) => right.recordedAtMs - left.recordedAtMs)
        .slice(0, 128);

      return {
        snapshot: {
          ...snapshot,
          auditEvents,
        },
        liveSnapshotRevision: state.liveSnapshotRevision + 1,
      };
    }),
  refresh: async (sessionId, pid) => {
    const requestId = ++snapshotRequestSeq;
    const sessionChanged = get().activeSessionId !== sessionId;
    const liveRevisionAtStart = get().liveSnapshotRevision;
    const nextActivePid = pid ?? (sessionChanged ? null : get().activePid);

    set((state) => ({
      activeSessionId: sessionId,
      activePid: nextActivePid,
      loading: true,
      error: null,
      snapshot: sessionChanged ? null : state.snapshot,
      timeline: sessionChanged ? null : state.timeline,
      timelineError: sessionChanged ? null : state.timelineError,
    }));

    try {
      const snapshot = await fetchWorkspaceSnapshot(sessionId, pid);
      const state = get();
      if (
        state.activeSessionId !== sessionId ||
        requestId !== snapshotRequestSeq ||
        state.liveSnapshotRevision !== liveRevisionAtStart
      ) {
        return;
      }

      set({
        snapshot,
        activePid: snapshot.activePid ?? snapshot.pid,
        loading: false,
        error: null,
      });
    } catch (error) {
      if (
        get().activeSessionId !== sessionId ||
        requestId !== snapshotRequestSeq
      ) {
        return;
      }

      set({
        loading: false,
        error:
          error instanceof Error
            ? error.message
            : "Failed to fetch workspace snapshot",
      });
    }
  },
  refreshTimeline: async (sessionId, pid) => {
    const requestId = ++timelineRequestSeq;
    const sessionChanged = get().activeSessionId !== sessionId;
    const liveRevisionAtStart = get().liveTimelineRevision;
    const nextActivePid = pid ?? (sessionChanged ? null : get().activePid);

    set((state) => ({
      activeSessionId: sessionId,
      activePid: nextActivePid,
      timelineLoading: true,
      timelineError: null,
      timeline: sessionChanged ? null : state.timeline,
      snapshot: sessionChanged ? null : state.snapshot,
      error: sessionChanged ? null : state.error,
    }));

    try {
      const timeline = await fetchTimelineSnapshot(sessionId, pid);
      const state = get();
      if (
        state.activeSessionId !== sessionId ||
        requestId !== timelineRequestSeq ||
        state.liveTimelineRevision !== liveRevisionAtStart
      ) {
        return;
      }

      set({
        timeline,
        activePid: timeline.pid,
        timelineLoading: false,
        timelineError: null,
      });
    } catch (error) {
      if (
        get().activeSessionId !== sessionId ||
        requestId !== timelineRequestSeq
      ) {
        return;
      }

      set({
        timelineLoading: false,
        timelineError:
          error instanceof Error
            ? error.message
            : "Failed to fetch timeline snapshot",
      });
    }
  },
  clear: () =>
    set({
      activeSessionId: null,
      activePid: null,
      snapshot: null,
      timeline: null,
      loading: false,
      timelineLoading: false,
      error: null,
      timelineError: null,
      liveSnapshotRevision: 0,
      liveTimelineRevision: 0,
    }),
}));
