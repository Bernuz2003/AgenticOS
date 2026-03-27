import { shortId } from "../utils/ids";
import { friendlyModelLabel, friendlyRuntimeLabel } from "./labels";

export function formatModelReference(modelId: string | null | undefined): string {
  return modelId ? friendlyModelLabel(modelId) : "n/a";
}

export function formatRuntimeReference(
  runtimeLabel: string | null | undefined,
  runtimeId: string | null | undefined,
): string {
  return friendlyRuntimeLabel(runtimeLabel ?? null, runtimeId ?? null);
}

export function formatBackendReference(
  backendClass: string | null | undefined,
  backendId: string | null | undefined,
): string {
  return backendClass || shortId(backendId) || "n/a";
}
