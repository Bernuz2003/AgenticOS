import { useSessionsStore } from "./sessions-store";

export function useJobsStore() {
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const scheduledJobs = useSessionsStore((state) => state.scheduledJobs);
  const refresh = useSessionsStore((state) => state.refresh);

  return { orchestrations, scheduledJobs, refresh };
}
