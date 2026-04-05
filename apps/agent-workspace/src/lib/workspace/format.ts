export function formatWorkspaceValue(
  value: number | string | boolean | null | undefined,
): string {
  if (value === null || value === undefined || value === "") {
    return "n/a";
  }
  if (typeof value === "number") {
    return value.toLocaleString();
  }
  if (typeof value === "boolean") {
    return value ? "yes" : "no";
  }
  return value;
}

export function formatLatencyMs(value: number | null | undefined): string {
  if (!Number.isFinite(value) || !value || value <= 0) {
    return "0 ms";
  }
  if (value < 1000) {
    return `${value} ms`;
  }
  return `${(value / 1000).toFixed(2)} s`;
}

export function formatTimestamp(
  value: number | null | undefined,
  options?: Intl.DateTimeFormatOptions,
): string {
  if (!Number.isFinite(value) || value === null || value === undefined) {
    return "n/a";
  }
  return new Date(value).toLocaleString("it-IT", options);
}

export function formatBytes(value: number | null | undefined): string {
  if (!Number.isFinite(value) || value === null || value === undefined || value < 0) {
    return "n/a";
  }
  if (value < 1024) {
    return `${value} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}
