import { useEffect, useMemo, useState } from "react";

import {
  captureCoreDump,
  fetchCoreDumpInfo,
  listCoreDumps,
  replayCoreDump,
  type CoreDumpInfo,
  type CoreDumpSummary,
  type ReplayCoreDumpResult,
} from "../lib/api";

interface UseCoreDumpsOptions {
  sessionId: string;
  pid: number | null;
  refreshKey: number;
  selectedDumpId: string | null;
}

export function useCoreDumps({
  sessionId,
  pid,
  refreshKey,
  selectedDumpId,
}: UseCoreDumpsOptions) {
  const [dumps, setDumps] = useState<CoreDumpSummary[]>([]);
  const [details, setDetails] = useState<Record<string, CoreDumpInfo>>({});
  const [loading, setLoading] = useState(false);
  const [capturePending, setCapturePending] = useState(false);
  const [replayPendingId, setReplayPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const relevantDumps = useMemo(
    () =>
      dumps
        .filter(
          (dump) => dump.sessionId === sessionId || (pid !== null && dump.pid === pid),
        )
        .sort((left, right) => right.createdAtMs - left.createdAtMs),
    [dumps, pid, sessionId],
  );

  const resolvedSelectedDumpId = useMemo(() => {
    if (
      selectedDumpId &&
      relevantDumps.some((dump) => dump.dumpId === selectedDumpId)
    ) {
      return selectedDumpId;
    }
    return relevantDumps[0]?.dumpId ?? null;
  }, [relevantDumps, selectedDumpId]);

  const selectedInfo = resolvedSelectedDumpId ? details[resolvedSelectedDumpId] ?? null : null;

  useEffect(() => {
    if (!sessionId) {
      return;
    }
    void refreshDumps();
  }, [pid, refreshKey, sessionId]);

  useEffect(() => {
    if (!resolvedSelectedDumpId) {
      return;
    }
    void ensureDumpInfo(resolvedSelectedDumpId);
  }, [resolvedSelectedDumpId]);

  async function refreshDumps() {
    setLoading(true);
    setError(null);
    try {
      const items = await listCoreDumps(48);
      setDumps(items);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to load core dumps");
    } finally {
      setLoading(false);
    }
  }

  async function ensureDumpInfo(dumpId: string) {
    if (details[dumpId]) {
      return details[dumpId];
    }
    try {
      const info = await fetchCoreDumpInfo(dumpId);
      setDetails((current) => ({ ...current, [dumpId]: info }));
      return info;
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to inspect core dump");
      return null;
    }
  }

  async function selectDump(dumpId: string) {
    setError(null);
    await ensureDumpInfo(dumpId);
  }

  async function captureDump() {
    if (pid === null) {
      return null;
    }

    setCapturePending(true);
    setError(null);
    try {
      const dump = await captureCoreDump({
        sessionId,
        pid,
        reason: "manual_workspace_capture",
      });
      await refreshDumps();
      await ensureDumpInfo(dump.dumpId);
      return dump;
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to capture core dump");
      return null;
    } finally {
      setCapturePending(false);
    }
  }

  async function replayDump(dumpId: string, branchLabel?: string | null) {
    setReplayPendingId(dumpId);
    setError(null);
    try {
      return await replayCoreDump(dumpId, branchLabel);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to launch replay branch");
      return null;
    } finally {
      setReplayPendingId(null);
    }
  }

  return {
    dumps: relevantDumps,
    resolvedSelectedDumpId,
    selectedInfo,
    loading,
    capturePending,
    replayPendingId,
    error,
    refreshDumps,
    selectDump,
    captureDump,
    replayDump,
  } satisfies {
    dumps: CoreDumpSummary[];
    resolvedSelectedDumpId: string | null;
    selectedInfo: CoreDumpInfo | null;
    loading: boolean;
    capturePending: boolean;
    replayPendingId: string | null;
    error: string | null;
    refreshDumps: () => Promise<void>;
    selectDump: (dumpId: string) => Promise<void>;
    captureDump: () => Promise<CoreDumpSummary | null>;
    replayDump: (
      dumpId: string,
      branchLabel?: string | null,
    ) => Promise<ReplayCoreDumpResult | null>;
  };
}
