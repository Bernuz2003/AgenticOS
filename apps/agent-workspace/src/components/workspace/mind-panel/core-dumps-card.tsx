import { useEffect, useMemo, useRef, useState } from "react";
import { Bug, Eye, RefreshCcw, RotateCcw, Save } from "lucide-react";

import {
  captureCoreDump,
  fetchCoreDumpInfo,
  listCoreDumps,
  replayCoreDump,
  type CoreDumpInfo,
  type CoreDumpSummary,
  type ReplayCoreDumpResult,
} from "../../../lib/api";
import { formatValue } from "./format";

interface CoreDumpsCardProps {
  sessionId: string;
  pid: number | null;
  refreshKey: number;
  onReplayReady: (result: ReplayCoreDumpResult) => void;
}

export function CoreDumpsCard({
  sessionId,
  pid,
  refreshKey,
  onReplayReady,
}: CoreDumpsCardProps) {
  const [dumps, setDumps] = useState<CoreDumpSummary[]>([]);
  const [selectedDumpId, setSelectedDumpId] = useState<string | null>(null);
  const [details, setDetails] = useState<Record<string, CoreDumpInfo>>({});
  const [loading, setLoading] = useState(false);
  const [capturePending, setCapturePending] = useState(false);
  const [replayPendingId, setReplayPendingId] = useState<string | null>(null);
  const [inspectPendingId, setInspectPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const detailRef = useRef<HTMLDivElement | null>(null);

  const visibleDumps = useMemo(
    () =>
      dumps
        .filter(
          (dump) =>
            dump.sessionId === sessionId || (pid !== null && dump.pid === pid),
        )
        .slice(0, 6),
    [dumps, pid, sessionId],
  );

  const selectedInfo =
    (selectedDumpId ? details[selectedDumpId] : null) ??
    (visibleDumps[0] ? details[visibleDumps[0].dumpId] : null) ??
    null;

  useEffect(() => {
    void refreshDumps();
  }, [pid, refreshKey, sessionId]);

  useEffect(() => {
    const defaultDumpId = visibleDumps[0]?.dumpId ?? null;
    if (!defaultDumpId) {
      setSelectedDumpId(null);
      return;
    }
    if (selectedDumpId && visibleDumps.some((dump) => dump.dumpId === selectedDumpId)) {
      return;
    }
    setSelectedDumpId(defaultDumpId);
    void ensureDumpInfo(defaultDumpId);
  }, [selectedDumpId, visibleDumps]);

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
      return;
    }
    try {
      const info = await fetchCoreDumpInfo(dumpId);
      setDetails((current) => ({ ...current, [dumpId]: info }));
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to inspect core dump");
    }
  }

  async function handleInspect(dumpId: string) {
    setInspectPendingId(dumpId);
    setError(null);
    setSelectedDumpId(dumpId);
    try {
      await ensureDumpInfo(dumpId);
      requestAnimationFrame(() => {
        detailRef.current?.scrollIntoView({
          behavior: "smooth",
          block: "nearest",
        });
      });
    } finally {
      setInspectPendingId(null);
    }
  }

  async function handleCapture() {
    if (pid === null) {
      return;
    }

    setCapturePending(true);
    setError(null);
    try {
      const dump = await captureCoreDump({
        sessionId,
        pid,
        reason: "manual_workspace_capture",
      });
      setSelectedDumpId(dump.dumpId);
      await refreshDumps();
      await ensureDumpInfo(dump.dumpId);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to capture core dump");
    } finally {
      setCapturePending(false);
    }
  }

  async function handleReplay(dumpId: string) {
    setReplayPendingId(dumpId);
    setError(null);
    try {
      const result = await replayCoreDump(dumpId);
      onReplayReady(result);
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : "Failed to launch replay branch");
    } finally {
      setReplayPendingId(null);
    }
  }

  return (
    <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-bold text-slate-900">
            <Bug className="h-4 w-4 text-amber-600" />
            Agent Core Dumps
          </div>
          <p className="mt-1 text-xs text-slate-500">
            Forensics, retention-aware history and replay launch for this session.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => void refreshDumps()}
            className="rounded-lg border border-slate-200 bg-white p-2 text-slate-600 shadow-sm transition-colors hover:border-indigo-200 hover:bg-indigo-50 hover:text-indigo-600"
            title="Refresh core dumps"
          >
            <RefreshCcw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </button>
          <button
            onClick={() => void handleCapture()}
            disabled={pid === null || capturePending}
            className="inline-flex items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs font-semibold text-amber-800 transition-colors hover:border-amber-300 hover:bg-amber-100 disabled:cursor-not-allowed disabled:border-slate-200 disabled:bg-slate-100 disabled:text-slate-400"
          >
            <Save className="h-4 w-4" />
            {capturePending ? "Capturing..." : "Dump now"}
          </button>
        </div>
      </div>

      <div className="space-y-3">
        {visibleDumps.length === 0 ? (
          <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-3 py-4 text-sm text-slate-500">
            No core dumps recorded yet for this session.
          </div>
        ) : (
          visibleDumps.map((dump) => {
            const isSelected = dump.dumpId === selectedDumpId;
            const replayPending = replayPendingId === dump.dumpId;
            const inspectPending = inspectPendingId === dump.dumpId;
            return (
              <div
                key={dump.dumpId}
                className={`rounded-xl border px-3 py-3 transition-colors ${
                  isSelected
                    ? "border-amber-200 bg-amber-50"
                    : "border-slate-200 bg-slate-50"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold text-slate-900">
                      {dump.reason}
                    </div>
                    <div className="mt-1 text-[11px] text-slate-500">
                      {new Date(dump.createdAtMs).toLocaleString("it-IT")}
                    </div>
                    <div className="mt-1 text-[11px] text-slate-500">
                      Fidelity: {dump.fidelity}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => void handleInspect(dump.dumpId)}
                      disabled={inspectPending}
                      className="inline-flex items-center gap-1 rounded-lg border border-slate-200 bg-white px-2 py-1 text-[11px] font-semibold text-slate-700 transition-colors hover:border-indigo-200 hover:bg-indigo-50 hover:text-indigo-700"
                    >
                      <Eye className="h-3.5 w-3.5" />
                      {inspectPending ? "Opening..." : "Inspect"}
                    </button>
                    <button
                      onClick={() => void handleReplay(dump.dumpId)}
                      disabled={replayPending}
                      className="inline-flex items-center gap-1 rounded-lg border border-emerald-200 bg-emerald-50 px-2 py-1 text-[11px] font-semibold text-emerald-800 transition-colors hover:border-emerald-300 hover:bg-emerald-100 disabled:cursor-not-allowed disabled:border-slate-200 disabled:bg-slate-100 disabled:text-slate-400"
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                      {replayPending ? "Launching..." : "Replay"}
                    </button>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {selectedInfo && (
        <div
          ref={detailRef}
          className="mt-4 rounded-2xl border border-slate-200 bg-slate-50 p-4"
        >
          <div className="mb-3 flex items-center justify-between gap-3">
            <div>
              <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
                Selected dump
              </div>
              <div className="mt-1 text-sm font-semibold text-slate-900">
                {selectedInfo.dump.dumpId}
              </div>
            </div>
            <span className="rounded-full border border-slate-200 bg-white px-3 py-1 text-[11px] font-bold text-slate-700">
              {selectedInfo.manifest.capture.reason ?? selectedInfo.dump.reason}
            </span>
          </div>
          <div className="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span className="mb-0.5 block text-slate-500">Target</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.target.source)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">State</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.target.state)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Replay messages</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.counts.replayMessages)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Checkpoints</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.counts.debugCheckpoints)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Invocations</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.counts.toolInvocations)}
              </span>
            </div>
            <div>
              <span className="mb-0.5 block text-slate-500">Workspace entries</span>
              <span className="font-medium text-slate-900">
                {formatValue(selectedInfo.manifest.counts.workspaceEntries)}
              </span>
            </div>
          </div>

          {selectedInfo.manifest.limitations.length > 0 && (
            <div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-xs text-amber-900">
              <div className="font-semibold">Replay limits</div>
              <div className="mt-1 leading-relaxed">
                {selectedInfo.manifest.limitations.slice(0, 4).join(", ")}
              </div>
            </div>
          )}
        </div>
      )}

      {error && (
        <div className="mt-4 rounded-xl border border-rose-200 bg-rose-50 px-3 py-3 text-sm text-rose-700">
          {error}
        </div>
      )}
    </section>
  );
}
