import { useJobsStore } from "../store/jobs-store";

export function useJobs() {
  return useJobsStore();
}
