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
  activePid: number | null;
  snapshot: WorkspaceSnapshot | null;
  timeline: TimelineSnapshot | null;
  loading: boolean;
  timelineLoading: boolean;
  error: string | null;
  timelineError: string | null;
  refresh: (pid: number) => Promise<void>;
  refreshTimeline: (pid: number) => Promise<void>;
  applySnapshot: (snapshot: WorkspaceSnapshot) => void;
  applyTimeline: (timeline: TimelineSnapshot) => void;
  clear: () => void;
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  activePid: null,
  snapshot: null,
  timeline: null,
  loading: false,
  timelineLoading: false,
  error: null,
  timelineError: null,
  applySnapshot: (snapshot) =>
    set((state) => {
      if (state.activePid !== null && state.activePid !== snapshot.pid) {
        return state;
      }

      return {
        activePid: snapshot.pid,
        snapshot,
        loading: false,
        error: null,
      };
    }),
  applyTimeline: (timeline) =>
    set((state) => {
      if (state.activePid !== null && state.activePid !== timeline.pid) {
        return state;
      }

      return {
        activePid: timeline.pid,
        timeline,
        timelineLoading: false,
        timelineError: null,
      };
    }),
  refresh: async (pid) => {
    const requestId = ++snapshotRequestSeq;
    const pidChanged = get().activePid !== pid;

    set((state) => ({
      activePid: pid,
      loading: true,
      error: null,
      snapshot: pidChanged ? null : state.snapshot,
      timeline: pidChanged ? null : state.timeline,
      timelineError: pidChanged ? null : state.timelineError,
    }));

    try {
      const snapshot = await fetchWorkspaceSnapshot(pid);
      if (get().activePid !== pid || requestId !== snapshotRequestSeq) {
        return;
      }

      set({ snapshot, loading: false, error: null });
    } catch (error) {
      if (get().activePid !== pid || requestId !== snapshotRequestSeq) {
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
  refreshTimeline: async (pid) => {
    const requestId = ++timelineRequestSeq;
    const pidChanged = get().activePid !== pid;

    set((state) => ({
      activePid: pid,
      timelineLoading: true,
      timelineError: null,
      timeline: pidChanged ? null : state.timeline,
      snapshot: pidChanged ? null : state.snapshot,
      error: pidChanged ? null : state.error,
    }));

    try {
      const timeline = await fetchTimelineSnapshot(pid);
      if (get().activePid !== pid || requestId !== timelineRequestSeq) {
        return;
      }

      set({ timeline, timelineLoading: false, timelineError: null });
    } catch (error) {
      if (get().activePid !== pid || requestId !== timelineRequestSeq) {
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
      activePid: null,
      snapshot: null,
      timeline: null,
      loading: false,
      timelineLoading: false,
      error: null,
      timelineError: null,
    }),
}));
