import type { OrchestrationStatus } from "../../lib/api";

export type InspectorTab =
  | "details"
  | "transcript"
  | "artifacts"
  | "events"
  | "messages";

export function formatTimestamp(timestampMs: number | null): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

export function formatElapsed(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 1) {
    return "<1s";
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`;
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatReasonLabel(reason: string | null | undefined): string {
  if (!reason) {
    return "n/a";
  }
  return reason.split("_").join(" ");
}

export function progressPercent(detail: OrchestrationStatus): number {
  if (detail.total <= 0) {
    return 0;
  }
  return Math.round(
    ((detail.completed + detail.failed + detail.skipped) / detail.total) * 100,
  );
}
