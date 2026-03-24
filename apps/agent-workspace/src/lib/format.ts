import type { SessionStatus } from "../store/sessions-store";

export function statusTone(status: SessionStatus): string {
  switch (status) {
    case "running":
      return "border-emerald-600/20 bg-emerald-50 text-emerald-700";
    case "swapped":
      return "border-amber-600/20 bg-amber-50 text-amber-700";
    case "idle":
    default:
      return "border-slate-900/10 bg-slate-100 text-slate-700";
  }
}

export function strategyLabel(strategy?: string | null): string {
  const normalized = strategy?.trim() || "sliding_window";
  return normalized.split("_").join(" ");
}

export function runtimeStateLabel(runtimeState?: string | null): string {
  const value = runtimeState?.trim();
  if (!value) {
    return "Unknown";
  }
  return value;
}

export function runtimeStateTone(runtimeState?: string | null): string {
  switch (runtimeState) {
    case "Running":
    case "InFlight":
    case "AwaitingRemoteResponse":
      return "border-emerald-600/20 bg-emerald-50 text-emerald-700";
    case "WaitingForSyscall":
    case "AwaitingTurnDecision":
      return "border-sky-600/20 bg-sky-50 text-sky-700";
    case "WaitingForHumanInput":
      return "border-cyan-600/20 bg-cyan-50 text-cyan-700";
    case "Parked":
      return "border-amber-600/20 bg-amber-50 text-amber-700";
    case "Killed":
    case "Errored":
    case "Terminated":
    case "Interrupted":
      return "border-rose-600/20 bg-rose-50 text-rose-700";
    case "WaitingForInput":
    case "Idle":
    case "Finished":
      return "border-slate-900/10 bg-slate-100 text-slate-700";
    default:
      return "border-violet-600/20 bg-violet-50 text-violet-700";
  }
}

export function localRuntimeStateLabel(state?: string | null): string {
  const value = state?.trim();
  if (!value) {
    return "unknown";
  }
  return value.replace(/_/g, " ");
}

export function localRuntimeStateTone(state?: string | null): string {
  switch (state) {
    case "ready":
      return "border-emerald-600/20 bg-emerald-50 text-emerald-700";
    case "starting":
    case "restarting":
      return "border-sky-600/20 bg-sky-50 text-sky-700";
    case "unhealthy":
    case "failed":
      return "border-rose-600/20 bg-rose-50 text-rose-700";
    case "external_override":
      return "border-amber-600/20 bg-amber-50 text-amber-700";
    default:
      return "border-slate-900/10 bg-slate-100 text-slate-700";
  }
}

export function deriveSessionStatus(
  runtimeState: string | null | undefined,
  timelineRunning: boolean,
): SessionStatus {
  if (runtimeState === "Parked") {
    return "swapped";
  }

  if (
    timelineRunning ||
    runtimeState === "Running" ||
    runtimeState === "WaitingForSyscall" ||
    runtimeState === "InFlight" ||
    runtimeState === "AwaitingRemoteResponse"
  ) {
    return "running";
  }

  return "idle";
}
