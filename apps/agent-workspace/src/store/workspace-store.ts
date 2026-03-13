import { create } from "zustand";

import {
  fetchTimelineSnapshot,
  fetchWorkspaceSnapshot,
  type TimelineSnapshot,
  type WorkspaceSnapshot,
} from "../lib/api";

let snapshotRequestSeq = 0;
let timelineRequestSeq = 0;

interface WorkspaceState {
  activeSessionId: string | null;
  activePid: number | null;
  snapshot: WorkspaceSnapshot | null;
  timeline: TimelineSnapshot | null;
  loading: boolean;
  timelineLoading: boolean;
  error: string | null;
  timelineError: string | null;
  refresh: (sessionId: string, pid: number | null) => Promise<void>;
  refreshTimeline: (sessionId: string, pid: number | null) => Promise<void>;
  applySnapshot: (snapshot: WorkspaceSnapshot) => void;
  applyTimeline: (timeline: TimelineSnapshot) => void;
  clear: () => void;
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
  applySnapshot: (snapshot) =>
    set((state) => {
      if (
        state.activeSessionId !== null &&
        state.activeSessionId !== snapshot.sessionId
      ) {
        return state;
      }

      return {
        activeSessionId: snapshot.sessionId,
        activePid: snapshot.pid,
        snapshot,
        loading: false,
        error: null,
      };
    }),
  applyTimeline: (timeline) =>
    set((state) => {
      if (
        state.activeSessionId !== null &&
        state.activeSessionId !== timeline.sessionId
      ) {
        return state;
      }

      return {
        activeSessionId: timeline.sessionId,
        activePid: timeline.pid,
        timeline,
        timelineLoading: false,
        timelineError: null,
      };
    }),
  refresh: async (sessionId, pid) => {
    const requestId = ++snapshotRequestSeq;
    const sessionChanged = get().activeSessionId !== sessionId;

    set((state) => ({
      activeSessionId: sessionId,
      activePid: pid,
      loading: true,
      error: null,
      snapshot: sessionChanged ? null : state.snapshot,
      timeline: sessionChanged ? null : state.timeline,
      timelineError: sessionChanged ? null : state.timelineError,
    }));

    try {
      const snapshot = await fetchWorkspaceSnapshot(sessionId, pid);
      if (
        get().activeSessionId !== sessionId ||
        requestId !== snapshotRequestSeq
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

    set((state) => ({
      activeSessionId: sessionId,
      activePid: pid,
      timelineLoading: true,
      timelineError: null,
      timeline: sessionChanged ? null : state.timeline,
      snapshot: sessionChanged ? null : state.snapshot,
      error: sessionChanged ? null : state.error,
    }));

    try {
      const timeline = await fetchTimelineSnapshot(sessionId, pid);
      if (
        get().activeSessionId !== sessionId ||
        requestId !== timelineRequestSeq
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
      activePid: null,
      snapshot: null,
      timeline: null,
      loading: false,
      timelineLoading: false,
      error: null,
      timelineError: null,
    }),
}));
