import { useWorkspaceStore } from "../store/workspace-store";

export function useWorkspace() {
  const snapshot = useWorkspaceStore((state) => state.snapshot);
  const timeline = useWorkspaceStore((state) => state.timeline);
  const loading = useWorkspaceStore((state) => state.loading);
  const timelineLoading = useWorkspaceStore((state) => state.timelineLoading);
  const error = useWorkspaceStore((state) => state.error);
  const timelineError = useWorkspaceStore((state) => state.timelineError);
  const refresh = useWorkspaceStore((state) => state.refresh);
  const refreshTimeline = useWorkspaceStore((state) => state.refreshTimeline);
  const clear = useWorkspaceStore((state) => state.clear);

  return {
    snapshot,
    timeline,
    loading,
    timelineLoading,
    error,
    timelineError,
    refresh,
    refreshTimeline,
    clear,
  };
}
