import { useSessionsStore } from "./sessions-store";

export function useModelsStore() {
  const selectedModelId = useSessionsStore((state) => state.selectedModelId);
  const loadedModelId = useSessionsStore((state) => state.loadedModelId);
  const loadedTargetKind = useSessionsStore((state) => state.loadedTargetKind);
  const loadedBackendId = useSessionsStore((state) => state.loadedBackendId);
  const loadedBackendClass = useSessionsStore((state) => state.loadedBackendClass);
  const refresh = useSessionsStore((state) => state.refresh);

  return {
    selectedModelId,
    loadedModelId,
    loadedTargetKind,
    loadedBackendId,
    loadedBackendClass,
    refresh,
  };
}
