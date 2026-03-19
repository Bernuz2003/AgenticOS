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

interface WorkspaceState {
  activeSessionId: string | null;
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

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  activeSessionId: null,
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
        state.activeSessionId !== null &&
        state.activeSessionId !== snapshot.sessionId
      ) {
        return state;
      }

      return {
        activeSessionId: snapshot.sessionId,
        snapshot,
        loading: false,
        error: null,
        liveSnapshotRevision: state.liveSnapshotRevision + 1,
      };
    }),
  applyLiveTimeline: (timeline) =>
    set((state) => {
      if (
        state.activeSessionId !== null &&
        state.activeSessionId !== timeline.sessionId
      ) {
        return state;
      }

      return {
        activeSessionId: timeline.sessionId,
        timeline,
        timelineLoading: false,
        timelineError: null,
        liveTimelineRevision: state.liveTimelineRevision + 1,
      };
    }),
  appendLiveAuditEvent: (event) =>
    set((state) => {
      const snapshot = state.snapshot;
      if (!snapshot) {
        return state;
      }

      const matchesSession =
        (event.sessionId !== null && snapshot.sessionId === event.sessionId) ||
        (event.pid !== null &&
          [snapshot.pid, snapshot.activePid, snapshot.lastPid].includes(event.pid));
      if (!matchesSession) {
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

    set((state) => ({
      activeSessionId: sessionId,
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

      set({ snapshot, loading: false, error: null });
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

    set((state) => ({
      activeSessionId: sessionId,
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

      set({ timeline, timelineLoading: false, timelineError: null });
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
