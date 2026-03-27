import {
  Activity,
  Cloud,
  Cpu,
  Layers3,
  Wrench,
  type LucideIcon,
} from "lucide-react";

import type { AuditEvent, OrchestrationStatus } from "../../lib/api";

export function formatBytes(bytes: number, decimals = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exponent = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / 1024 ** exponent;
  return `${value.toFixed(exponent === 0 ? 0 : decimals)} ${units[exponent]}`;
}

export function formatRelative(timestampMs: number): string {
  const delta = Math.max(0, Date.now() - timestampMs);
  if (delta < 1000) {
    return "now";
  }
  if (delta < 60_000) {
    return `${Math.floor(delta / 1000)}s ago`;
  }
  if (delta < 3_600_000) {
    return `${Math.floor(delta / 60_000)}m ago`;
  }
  return `${Math.floor(delta / 3_600_000)}h ago`;
}

export function formatPercent(used: number, total: number): string {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) {
    return "0%";
  }
  return `${Math.min(100, Math.round((used / total) * 100))}%`;
}

export function formatLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "0 ms";
  }
  if (ms < 1000) {
    return `${Math.round(ms)} ms`;
  }
  return `${(ms / 1000).toFixed(2)} s`;
}

export function formatScore(score: number | null): string {
  if (score === null || !Number.isFinite(score)) {
    return "n/a";
  }
  return score.toFixed(3);
}

export function taskStatusTone(status: string): string {
  switch (status) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "failed":
      return "border-rose-200 bg-rose-50 text-rose-700";
    case "skipped":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

export function diagnosticTone(category: string): string {
  switch (category) {
    case "tool":
      return "border-indigo-200 bg-indigo-50 text-indigo-700";
    case "remote":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "runtime":
    case "admission":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "process":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

export function categoryIcon(category: string): LucideIcon {
  switch (category) {
    case "tool":
      return Wrench;
    case "remote":
      return Cloud;
    case "runtime":
    case "admission":
      return Cpu;
    case "process":
      return Activity;
    default:
      return Layers3;
  }
}

export function workflowArtifactCount(detail: OrchestrationStatus): number {
  return detail.tasks.reduce((count, task) => count + task.outputArtifacts.length, 0);
}

export function workflowSessionCount(detail: OrchestrationStatus): number {
  const sessions = new Set<string>();
  for (const task of detail.tasks) {
    for (const attempt of task.attempts) {
      if (attempt.sessionId) {
        sessions.add(attempt.sessionId);
      }
    }
  }
  return sessions.size;
}

export function buildCategoryCounts(events: AuditEvent[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const event of events) {
    counts.set(event.category, (counts.get(event.category) ?? 0) + 1);
  }
  return counts;
}
