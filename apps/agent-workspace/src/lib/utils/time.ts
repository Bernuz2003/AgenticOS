export function formatTimestampLabel(timestampMs: number | null | undefined): string {
  if (!timestampMs) {
    return "n/a";
  }
  return new Date(timestampMs).toLocaleString();
}

export function formatRelativeTime(timestampMs: number | null | undefined): string {
  if (!timestampMs) {
    return "n/a";
  }
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
