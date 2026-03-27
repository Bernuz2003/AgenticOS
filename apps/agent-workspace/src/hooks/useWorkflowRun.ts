import { useSessionsStore } from "../store/sessions-store";

export function useWorkflowRunStore() {
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const refresh = useSessionsStore((state) => state.refresh);

  return { orchestrations, refresh };
}
