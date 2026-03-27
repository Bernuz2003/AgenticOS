import { useSessionsStore } from "./sessions-store";

export function useWorkflowStore() {
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const refresh = useSessionsStore((state) => state.refresh);

  return { orchestrations, refresh };
}
