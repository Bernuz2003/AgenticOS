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
