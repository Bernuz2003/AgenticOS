import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  Activity,
  ArrowRight,
  Cloud,
  Cpu,
  Database,
  Layers3,
  ShieldAlert,
  Wrench,
} from "lucide-react";
import {
  fetchOrchestrationStatus,
  type AuditEvent,
  type OrchestrationStatus,
} from "../lib/api";
import {
  runtimeStateLabel,
  runtimeStateTone,
  statusTone,
  strategyLabel,
} from "../lib/format";
import { friendlyModelLabel, friendlyRuntimeLabel } from "../lib/model-labels";
import { useSessionsStore } from "../store/sessions-store";

function formatBytes(bytes: number, decimals = 1): string {
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

function formatRelative(timestampMs: number): string {
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

function formatPercent(used: number, total: number): string {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) {
    return "0%";
  }
  return `${Math.min(100, Math.round((used / total) * 100))}%`;
}

function formatLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "0 ms";
  }
  if (ms < 1000) {
    return `${Math.round(ms)} ms`;
  }
  return `${(ms / 1000).toFixed(2)} s`;
}

function formatScore(score: number | null): string {
  if (score === null || !Number.isFinite(score)) {
    return "n/a";
  }
  return score.toFixed(3);
}

function taskStatusTone(status: string): string {
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

function diagnosticTone(category: string): string {
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

function categoryIcon(category: string) {
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

function workflowArtifactCount(detail: OrchestrationStatus): number {
  return detail.tasks.reduce(
    (count, task) => count + task.outputArtifacts.length,
    0,
  );
}

function workflowSessionCount(detail: OrchestrationStatus): number {
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

export function ControlCenterPage() {
  const {
    connected,
    error,
    sessions,
    orchestrations,
    selectedModelId,
    loadedModelId,
    loadedTargetKind,
    loadedProviderId,
    loadedRemoteModelId,
    loadedBackendId,
    loadedBackendClass,
    loadedBackendCapabilities,
    loadedBackendTelemetry,
    loadedRemoteModel,
    globalAccounting,
    memory,
    runtimeInstances,
    resourceGovernor,
    runtimeLoadQueue,
    globalAuditEvents,
    scheduledJobs,
    refresh,
  } = useSessionsStore();
  const [workflowDetails, setWorkflowDetails] = useState<
    Record<number, OrchestrationStatus>
  >({});
  const [workflowError, setWorkflowError] = useState<string | null>(null);
  const [selectedCategory, setSelectedCategory] = useState<string>("all");

  const orchestrationSignature = useMemo(
    () =>
      orchestrations
        .map(
          (workflow) =>
            `${workflow.orchestrationId}:${workflow.running}:${workflow.pending}:${workflow.completed}:${workflow.failed}:${workflow.skipped}`,
        )
        .join("|"),
    [orchestrations],
  );

  useEffect(() => {
    if (orchestrations.length === 0) {
      setWorkflowDetails({});
      setWorkflowError(null);
      return;
    }

    let cancelled = false;

    const load = async () => {
      const results = await Promise.allSettled(
        orchestrations.map(async (workflow) => [
          workflow.orchestrationId,
          await fetchOrchestrationStatus(workflow.orchestrationId),
        ] as const),
      );

      if (cancelled) {
        return;
      }

      const nextDetails: Record<number, OrchestrationStatus> = {};
      let firstError: string | null = null;

      for (const result of results) {
        if (result.status === "fulfilled") {
          const [orchestrationId, detail] = result.value;
          nextDetails[orchestrationId] = detail;
          continue;
        }

        if (!firstError) {
          firstError =
            result.reason instanceof Error
              ? result.reason.message
              : "Failed to fetch workflow details";
        }
      }

      setWorkflowDetails(nextDetails);
      setWorkflowError(firstError);
    };

    void load();

    return () => {
      cancelled = true;
    };
  }, [orchestrationSignature, orchestrations]);

  const runningSessions = sessions.filter((session) => session.status === "running");
  const parkedSessions = sessions.filter((session) => session.status === "swapped");
  const idleSessions = sessions.filter((session) => session.status === "idle");
  const runningOrchestrations = orchestrations.filter(
    (workflow) => !workflow.finished,
  );
  const failedOrchestrations = orchestrations.filter(
    (workflow) => workflow.failed > 0,
  );
  const activeScheduledJobs = scheduledJobs.filter(
    (job) => job.state === "running" || job.state === "retry_wait",
  );
  const categoryCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const event of globalAuditEvents) {
      counts.set(event.category, (counts.get(event.category) ?? 0) + 1);
    }
    return counts;
  }, [globalAuditEvents]);
  const categoryOptions = useMemo(
    () => ["all", ...Array.from(categoryCounts.keys()).sort()],
    [categoryCounts],
  );
  const filteredAuditEvents = useMemo(() => {
    if (selectedCategory === "all") {
      return globalAuditEvents;
    }
    return globalAuditEvents.filter((event) => event.category === selectedCategory);
  }, [globalAuditEvents, selectedCategory]);
  const diagnosticHighlights = useMemo(() => {
    const categories = ["tool", "remote", "runtime", "process"];
    return categories.map((category) => {
      const events = globalAuditEvents.filter((event) =>
        category === "runtime"
          ? event.category === "runtime" || event.category === "admission"
          : event.category === category,
      );
      return {
        category,
        count: events.length,
        latest: events[0] ?? null,
      };
    });
  }, [globalAuditEvents]);

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="rounded-3xl border border-slate-200 bg-white px-8 py-7 shadow-sm">
        <div className="flex flex-col gap-5 xl:flex-row xl:items-end xl:justify-between">
          <div className="max-w-4xl">
            <div className="text-xs font-bold uppercase tracking-[0.25em] text-slate-400">
              Runtime Control Center
            </div>
            <h1 className="mt-2 text-3xl font-bold tracking-tight text-slate-900">
              Deep Observability
            </h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              A single operator view for process state, workflow execution,
              runtime/backend health, in-flight load queue and live diagnostics.
              Chat stays focused on conversation; technical traffic lives here.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Link
              to="/sessions"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Open Chats
            </Link>
            <Link
              to="/workflows"
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 hover:bg-slate-50"
            >
              Open Workflows
            </Link>
            <button
              onClick={() => void refresh()}
              className="rounded-xl bg-slate-900 px-5 py-2.5 text-sm font-semibold text-white hover:bg-slate-800"
            >
              Refresh Snapshot
            </button>
          </div>
        </div>
      </header>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <section className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Bridge Status
          </div>
          <div className="mt-3 flex items-center gap-3">
            <div
              className={`h-3 w-3 rounded-full ${
                connected ? "bg-emerald-500" : "bg-rose-500"
              }`}
            />
            <div className="text-lg font-bold text-slate-900">
              {connected ? "Connected" : "Disconnected"}
            </div>
          </div>
          <div className="mt-2 text-sm text-slate-500">
            {error ? error : "Lobby snapshots and live diagnostics are flowing."}
          </div>
        </section>

        <section className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Runtime Target
          </div>
          <div className="mt-3 text-lg font-bold text-slate-900">
            {loadedModelId ? friendlyModelLabel(loadedModelId) : "No model resident"}
          </div>
          <div className="mt-2 text-sm text-slate-500">
            {loadedTargetKind
              ? `${loadedTargetKind} via ${loadedBackendClass || loadedBackendId || "unknown"}`
              : `Selected model: ${selectedModelId ? friendlyModelLabel(selectedModelId) : "none"}`}
          </div>
        </section>

        <section className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Live Processes
          </div>
          <div className="mt-3 text-lg font-bold text-slate-900">
            {sessions.length} sessions
          </div>
          <div className="mt-2 text-sm text-slate-500">
            {runningSessions.length} running, {parkedSessions.length} parked,{" "}
            {idleSessions.length} idle.
          </div>
        </section>

        <section className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Workflow Ops
          </div>
          <div className="mt-3 text-lg font-bold text-slate-900">
            {orchestrations.length} orchestrations / {scheduledJobs.length} jobs
          </div>
          <div className="mt-2 text-sm text-slate-500">
            {runningOrchestrations.length} active workflows,{" "}
            {activeScheduledJobs.length} active scheduler jobs,{" "}
            {failedOrchestrations.length} workflows with failures.
          </div>
        </section>
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Runtime & Backend
              </div>
              <h2 className="mt-2 text-xl font-bold text-slate-900">
                Execution target, provider and telemetry
              </h2>
            </div>
            <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
              {runtimeInstances.length} resident runtimes
            </div>
          </div>

          <div className="mt-6 grid gap-4 md:grid-cols-2">
            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
                Loaded backend
              </div>
              <div className="mt-2 text-lg font-semibold text-slate-900">
                {loadedBackendClass || "n/a"}
              </div>
              <div className="mt-1 text-sm text-slate-500">
                backend_id: {loadedBackendId || "n/a"}
              </div>
              <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                <div>
                  <div className="text-slate-500">Provider</div>
                  <div className="font-medium text-slate-900">
                    {loadedProviderId || loadedRemoteModel?.providerLabel || "local"}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Remote model</div>
                  <div className="font-medium text-slate-900">
                    {loadedRemoteModelId
                      ? friendlyModelLabel(loadedRemoteModelId)
                      : loadedRemoteModel?.modelLabel || "n/a"}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Target kind</div>
                  <div className="font-medium text-slate-900">
                    {loadedTargetKind || "n/a"}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Structured output</div>
                  <div className="font-medium text-slate-900">
                    {loadedBackendCapabilities?.structuredOutput ? "yes" : "no"}
                  </div>
                </div>
              </div>
            </div>

            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
                Backend telemetry
              </div>
              <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                <div>
                  <div className="text-slate-500">Requests</div>
                  <div className="font-medium text-slate-900">
                    {loadedBackendTelemetry
                      ? loadedBackendTelemetry.requestsTotal +
                        loadedBackendTelemetry.streamRequestsTotal
                      : 0}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Estimated cost</div>
                  <div className="font-medium text-slate-900">
                    $
                    {(
                      loadedBackendTelemetry?.estimatedCostUsd ??
                      globalAccounting?.estimatedCostUsd ??
                      0
                    ).toFixed(4)}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Input tokens</div>
                  <div className="font-medium text-slate-900">
                    {(loadedBackendTelemetry?.inputTokensTotal ?? 0).toLocaleString()}
                  </div>
                </div>
                <div>
                  <div className="text-slate-500">Output tokens</div>
                  <div className="font-medium text-slate-900">
                    {(loadedBackendTelemetry?.outputTokensTotal ?? 0).toLocaleString()}
                  </div>
                </div>
              </div>
              <div className="mt-4 rounded-xl border border-slate-200 bg-white px-3 py-3 text-xs text-slate-600">
                Last remote error:{" "}
                <span className="font-medium text-slate-900">
                  {loadedBackendTelemetry?.lastError || "none"}
                </span>
              </div>
            </div>
          </div>

          <div className="mt-5 flex flex-wrap gap-2">
            {loadedBackendCapabilities &&
            Object.entries(loadedBackendCapabilities).length > 0 ? (
              Object.entries(loadedBackendCapabilities).map(([key, enabled]) => (
                <span
                  key={key}
                  className={`rounded-full border px-3 py-1 text-[11px] font-bold uppercase tracking-wider ${
                    enabled
                      ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                      : "border-slate-200 bg-slate-100 text-slate-500"
                  }`}
                >
                  {key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)}
                </span>
              ))
            ) : (
              <div className="text-sm text-slate-500">
                No backend capability snapshot available.
              </div>
            )}
          </div>

          <div className="mt-6 space-y-3">
            {runtimeInstances.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-center text-sm text-slate-500">
                No resident runtime is currently loaded.
              </div>
            ) : (
              runtimeInstances.map((runtime) => (
                <div
                  key={runtime.runtimeId}
                  className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4"
                >
                  <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                    <div>
                      <div className="flex flex-wrap items-center gap-2">
                        <div className="text-sm font-semibold text-slate-900">
                          {friendlyModelLabel(runtime.logicalModelId)}
                        </div>
                        <span
                          className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${
                            runtime.current
                              ? "border-indigo-200 bg-indigo-50 text-indigo-700"
                              : "border-slate-200 bg-white text-slate-600"
                          }`}
                        >
                          {runtime.current ? "current" : runtime.state}
                        </span>
                        {runtime.transitionState ? (
                          <span className="rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-amber-700">
                            {runtime.transitionState}
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-1 text-xs text-slate-500">
                        runtime_id: {runtime.runtimeId} · backend: {runtime.backendClass}
                      </div>
                    </div>
                    <div className="grid grid-cols-3 gap-3 text-right text-xs">
                      <div>
                        <div className="text-slate-500">Active PIDs</div>
                        <div className="font-medium text-slate-900">
                          {runtime.activePidCount}
                        </div>
                      </div>
                      <div>
                        <div className="text-slate-500">VRAM</div>
                        <div className="font-medium text-slate-900">
                          {formatBytes(runtime.reservationVramBytes)}
                        </div>
                      </div>
                      <div>
                        <div className="text-slate-500">RAM</div>
                        <div className="font-medium text-slate-900">
                          {formatBytes(runtime.reservationRamBytes)}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                Queue & Residency
              </div>
              <h2 className="mt-2 text-xl font-bold text-slate-900">
                Resource governor, paging and in-flight load queue
              </h2>
            </div>
            <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
              {runtimeLoadQueue.length} queued loads
            </div>
          </div>

          <div className="mt-6 grid gap-4 md:grid-cols-2">
            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                <ShieldAlert className="h-4 w-4 text-indigo-500" />
                Resource governor
              </div>
              {!resourceGovernor ? (
                <div className="mt-4 text-sm text-slate-500">
                  Waiting for governor telemetry...
                </div>
              ) : (
                <div className="mt-4 space-y-4">
                  <div>
                    <div className="mb-1.5 flex items-center justify-between text-xs text-slate-500">
                      <span>VRAM usage</span>
                      <span>
                        {formatBytes(resourceGovernor.vramUsedBytes)} /{" "}
                        {formatBytes(resourceGovernor.vramBudgetBytes)}
                      </span>
                    </div>
                    <div className="h-2.5 overflow-hidden rounded-full bg-slate-200">
                      <div
                        className="h-full rounded-full bg-indigo-500"
                        style={{
                          width: formatPercent(
                            resourceGovernor.vramUsedBytes,
                            resourceGovernor.vramBudgetBytes,
                          ),
                        }}
                      />
                    </div>
                  </div>

                  <div>
                    <div className="mb-1.5 flex items-center justify-between text-xs text-slate-500">
                      <span>RAM usage</span>
                      <span>
                        {formatBytes(resourceGovernor.ramUsedBytes)} /{" "}
                        {formatBytes(resourceGovernor.ramBudgetBytes)}
                      </span>
                    </div>
                    <div className="h-2.5 overflow-hidden rounded-full bg-slate-200">
                      <div
                        className="h-full rounded-full bg-emerald-500"
                        style={{
                          width: formatPercent(
                            resourceGovernor.ramUsedBytes,
                            resourceGovernor.ramBudgetBytes,
                          ),
                        }}
                      />
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3 text-xs">
                    <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                      <div className="text-slate-500">Pending queue</div>
                      <div className="mt-1 font-medium text-slate-900">
                        {resourceGovernor.pendingQueueDepth}
                      </div>
                    </div>
                    <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                      <div className="text-slate-500">Loader</div>
                      <div className="mt-1 font-medium text-slate-900">
                        {resourceGovernor.loaderBusy
                          ? resourceGovernor.loaderReason || "busy"
                          : "idle"}
                      </div>
                    </div>
                  </div>
                </div>
              )}
            </div>

            <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
              <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                <Database className="h-4 w-4 text-rose-500" />
                Virtual memory
              </div>
              {!memory ? (
                <div className="mt-4 text-sm text-slate-500">
                  Waiting for memory telemetry...
                </div>
              ) : (
                <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Tracked PIDs</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {memory.trackedPids}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Parked PIDs</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {memory.parkedPids}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Free blocks</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {memory.freeBlocks} / {memory.totalBlocks}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Allocated</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {formatBytes(memory.allocBytes)}
                    </div>
                  </div>
                  <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-3">
                    <div className="text-amber-600">Swap faults</div>
                    <div className="mt-1 font-medium text-amber-800">
                      {memory.swapFaults}
                    </div>
                  </div>
                  <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-3">
                    <div className="text-rose-600">OOM events</div>
                    <div className="mt-1 font-medium text-rose-800">
                      {memory.oomEvents}
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>

          <div className="mt-5 space-y-3">
            {runtimeLoadQueue.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-center text-sm text-slate-500">
                No pending runtime load requests.
              </div>
            ) : (
              runtimeLoadQueue.map((entry) => (
                <div
                  key={entry.queueId}
                  className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4"
                >
                  <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">
                        {friendlyModelLabel(entry.logicalModelId)}
                      </div>
                      <div className="mt-1 text-xs text-slate-500">
                        {entry.backendClass} · {entry.reason}
                      </div>
                    </div>
                    <div className="grid grid-cols-3 gap-3 text-right text-xs">
                      <div>
                        <div className="text-slate-500">State</div>
                        <div className="font-medium text-slate-900">
                          {entry.state}
                        </div>
                      </div>
                      <div>
                        <div className="text-slate-500">Requested</div>
                        <div className="font-medium text-slate-900">
                          {formatRelative(entry.requestedAtMs)}
                        </div>
                      </div>
                      <div>
                        <div className="text-slate-500">VRAM</div>
                        <div className="font-medium text-slate-900">
                          {formatBytes(entry.reservationVramBytes)}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              ))
            )}
          </div>
        </section>
      </div>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Process Board
            </div>
            <h2 className="mt-2 text-xl font-bold text-slate-900">
              Explicit process state and workspace routing
            </h2>
            <p className="mt-2 text-sm text-slate-500">
              Interactive chat sessions stay visible with both UI status and live
              runtime state, so parked, idle and active processes are not conflated.
            </p>
          </div>
          <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
            {sessions.length} sessions
          </div>
        </div>

        {sessions.length === 0 ? (
          <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center">
            <div className="text-lg font-semibold text-slate-800">
              No sessions yet
            </div>
            <div className="mt-2 text-sm text-slate-500">
              Start a chat to populate the process board. Workflow task execution is
              monitored from Jobs and workflow run detail.
            </div>
          </div>
        ) : (
          <div className="mt-6 grid gap-4 xl:grid-cols-2">
            {sessions.map((session) => (
              <div
                key={session.sessionId}
                className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
              >
                <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="truncate text-sm font-semibold text-slate-900">
                        {session.title}
                      </div>
                      <span
                        className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${statusTone(session.status)}`}
                      >
                        {session.status}
                      </span>
                      <span
                        className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${runtimeStateTone(session.runtimeState)}`}
                      >
                        {runtimeStateLabel(session.runtimeState)}
                      </span>
                    </div>
                    <div className="mt-1 text-xs text-slate-500">
                      session_id: {session.sessionId}
                    </div>
                    <div className="mt-3 text-sm text-slate-600">
                      {session.promptPreview}
                    </div>
                  </div>
                  <Link
                    to={`/workspace/${session.sessionId}`}
                    className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-100"
                  >
                    Open Workspace
                    <ArrowRight className="h-4 w-4" />
                  </Link>
                </div>

                <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">PID routing</div>
                    <div className="mt-1 font-medium text-slate-900">
                      active {session.activePid ?? "n/a"} · last {session.lastPid ?? "n/a"}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Runtime</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {friendlyRuntimeLabel(session.runtimeLabel, session.runtimeId)}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Backend</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {session.backendClass || "n/a"}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Context strategy</div>
                    <div className="mt-1 font-medium capitalize text-slate-900">
                      {strategyLabel(session.contextStrategy)}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Uptime</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {session.uptimeLabel}
                    </div>
                  </div>
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                    <div className="text-slate-500">Tokens</div>
                    <div className="mt-1 font-medium text-slate-900">
                      {session.tokensLabel}
                    </div>
                  </div>
                </div>

                {session.orchestrationId ? (
                  <div className="mt-4 rounded-xl border border-indigo-200 bg-indigo-50 px-4 py-3 text-xs text-indigo-800">
                    workflow {session.orchestrationId}
                    {session.orchestrationTaskId
                      ? ` · task ${session.orchestrationTaskId}`
                      : ""}
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Workflow Execution
            </div>
            <h2 className="mt-2 text-xl font-bold text-slate-900">
              Readable orchestration monitor with task attempts and artifacts
            </h2>
          </div>
          <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
            {orchestrations.length} workflows
          </div>
        </div>

        {workflowError ? (
          <div className="mt-5 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-700">
            {workflowError}
          </div>
        ) : null}

        {orchestrations.length === 0 ? (
          <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center">
            <div className="text-lg font-semibold text-slate-800">
              No active workflows
            </div>
            <div className="mt-2 text-sm text-slate-500">
              Launch a workflow to observe task trees, attempts and artifacts here.
            </div>
          </div>
        ) : (
          <div className="mt-6 space-y-4">
            {orchestrations.map((workflow) => {
              const detail = workflowDetails[workflow.orchestrationId];
              return (
                <div
                  key={workflow.orchestrationId}
                  className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
                >
                  <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div>
                      <div className="flex flex-wrap items-center gap-2">
                        <div className="text-sm font-semibold text-slate-900">
                          Workflow {workflow.orchestrationId}
                        </div>
                        <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-indigo-700">
                          {workflow.policy}
                        </span>
                        {workflow.finished ? (
                          <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                            finished
                          </span>
                        ) : null}
                      </div>
                      <div className="mt-2 text-xs text-slate-500">
                        elapsed {workflow.elapsedLabel}
                        {detail
                          ? ` · ${workflowSessionCount(detail)} workspaces · ${workflowArtifactCount(detail)} artifacts`
                          : ""}
                      </div>
                    </div>
                    <Link
                      to="/workflows"
                      className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-100"
                    >
                      Open Workflow Console
                      <ArrowRight className="h-4 w-4" />
                    </Link>
                  </div>

                  <div className="mt-4 grid grid-cols-5 gap-2 text-center text-xs">
                    <div className="rounded-xl border border-slate-200 bg-white px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wider text-slate-400">
                        Total
                      </div>
                      <div className="mt-1 font-bold text-slate-900">{workflow.total}</div>
                    </div>
                    <div className="rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wider text-emerald-500">
                        Run
                      </div>
                      <div className="mt-1 font-bold text-emerald-700">
                        {workflow.running}
                      </div>
                    </div>
                    <div className="rounded-xl border border-sky-200 bg-sky-50 px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wider text-sky-500">
                        Done
                      </div>
                      <div className="mt-1 font-bold text-sky-700">
                        {workflow.completed}
                      </div>
                    </div>
                    <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wider text-amber-500">
                        Wait
                      </div>
                      <div className="mt-1 font-bold text-amber-700">
                        {workflow.pending}
                      </div>
                    </div>
                    <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wider text-rose-500">
                        Fail
                      </div>
                      <div className="mt-1 font-bold text-rose-700">
                        {workflow.failed}
                      </div>
                    </div>
                  </div>

                  {!detail ? (
                    <div className="mt-4 text-sm text-slate-500">
                      Loading workflow detail...
                    </div>
                  ) : (
                    <div className="mt-5 grid gap-3 xl:grid-cols-2">
                      {detail.tasks.map((task) => {
                        const liveAttempt = task.attempts.find(
                          (attempt) => attempt.attempt === task.currentAttempt,
                        );
                        const sessionId =
                          liveAttempt?.sessionId ??
                          task.attempts.find((attempt) => attempt.sessionId)?.sessionId ??
                          null;
                        return (
                          <div
                            key={`${detail.orchestrationId}-${task.task}`}
                            className="rounded-2xl border border-slate-200 bg-white p-4"
                          >
                            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                              <div className="min-w-0 flex-1">
                                <div className="flex flex-wrap items-center gap-2">
                                  <div className="text-sm font-semibold text-slate-900">
                                    {task.task}
                                  </div>
                                  <span
                                    className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskStatusTone(task.status)}`}
                                  >
                                    {task.status}
                                  </span>
                                  {task.currentAttempt ? (
                                    <span className="rounded-full border border-slate-200 bg-slate-100 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                                      attempt {task.currentAttempt}
                                    </span>
                                  ) : null}
                                </div>
                                <div className="mt-2 text-xs text-slate-500">
                                  role {task.role || "n/a"} · workload{" "}
                                  {task.workload || "default"} · backend{" "}
                                  {task.backendClass || "auto"}
                                </div>
                              </div>
                              {sessionId ? (
                                <Link
                                  to={`/workspace/${sessionId}`}
                                  className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs font-semibold text-slate-700 hover:bg-slate-100"
                                >
                                  Open Task Workspace
                                  <ArrowRight className="h-3.5 w-3.5" />
                                </Link>
                              ) : null}
                            </div>

                            <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                              <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                                <div className="text-slate-500">Dependencies</div>
                                <div className="mt-1 font-medium text-slate-900">
                                  {task.deps.length > 0 ? task.deps.join(", ") : "root"}
                                </div>
                              </div>
                              <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                                <div className="text-slate-500">Attempts</div>
                                <div className="mt-1 font-medium text-slate-900">
                                  {task.attempts.length}
                                </div>
                              </div>
                              <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                                <div className="text-slate-500">Input artifacts</div>
                                <div className="mt-1 font-medium text-slate-900">
                                  {task.inputArtifacts.length}
                                </div>
                              </div>
                              <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                                <div className="text-slate-500">Output artifacts</div>
                                <div className="mt-1 font-medium text-slate-900">
                                  {task.outputArtifacts.length}
                                </div>
                              </div>
                            </div>

                            {task.latestOutputPreview ? (
                              <div className="mt-4 rounded-xl border border-slate-200 bg-slate-50 px-4 py-3 text-xs leading-6 text-slate-600">
                                {task.latestOutputPreview}
                              </div>
                            ) : null}

                            {task.context ? (
                              <div className="mt-4 rounded-xl border border-indigo-200 bg-indigo-50/60 px-4 py-4">
                                <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-indigo-700">
                                  Semantic Retrieval
                                </div>
                                <div className="mt-3 grid grid-cols-2 gap-3 text-xs">
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Strategy</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {strategyLabel(task.context.contextStrategy)}
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Episodic Memory</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.episodicSegments} segments ·{" "}
                                      {task.context.episodicTokens} tokens
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Requests / Hits / Misses</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.contextRetrievalRequests} /{" "}
                                      {task.context.contextRetrievalHits} /{" "}
                                      {task.context.contextRetrievalMisses}
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Candidates / Selected</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.contextRetrievalCandidatesScored} /{" "}
                                      {task.context.contextRetrievalSegmentsSelected}
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Last Scan</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.lastRetrievalCandidatesScored} scored ·{" "}
                                      {task.context.lastRetrievalSegmentsSelected} kept
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Latency / Top Score</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {formatLatency(task.context.lastRetrievalLatencyMs)} ·{" "}
                                      {formatScore(task.context.lastRetrievalTopScore)}
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Top K / Candidate Limit</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.retrieveTopK} /{" "}
                                      {task.context.retrieveCandidateLimit}
                                    </div>
                                  </div>
                                  <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                    <div className="text-indigo-500">Min Score / Max Chars</div>
                                    <div className="mt-1 font-semibold text-slate-900">
                                      {task.context.retrieveMinScore.toFixed(2)} /{" "}
                                      {task.context.retrieveMaxSegmentChars}
                                    </div>
                                  </div>
                                </div>
                              </div>
                            ) : null}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
              Diagnostics Console
            </div>
            <h2 className="mt-2 text-xl font-bold text-slate-900">
              Tool plane, remote requests, runtime transitions and errors
            </h2>
            <p className="mt-2 text-sm text-slate-500">
              Live audit events remain separate from the chat timeline and are
              grouped here for operator debugging.
            </p>
          </div>
          <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
            {globalAuditEvents.length} events
          </div>
        </div>

        <div className="mt-6 grid gap-3 md:grid-cols-4">
          {diagnosticHighlights.map(({ category, count, latest }) => {
            const Icon = categoryIcon(category);
            return (
              <div
                key={category}
                className={`rounded-2xl border px-4 py-4 ${diagnosticTone(category)}`}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider">
                    <Icon className="h-4 w-4" />
                    {category}
                  </div>
                  <div className="rounded-full border border-current/20 bg-white/60 px-2 py-0.5 text-[10px] font-bold">
                    {count}
                  </div>
                </div>
                <div className="mt-3 text-sm font-semibold">
                  {latest ? latest.title : "No events yet"}
                </div>
                <div className="mt-1 text-xs opacity-80">
                  {latest ? formatRelative(latest.recordedAtMs) : "Awaiting signal"}
                </div>
              </div>
            );
          })}
        </div>

        <div className="mt-6 flex flex-wrap gap-2">
          {categoryOptions.map((category) => {
            const active = selectedCategory === category;
            const count =
              category === "all"
                ? globalAuditEvents.length
                : (categoryCounts.get(category) ?? 0);
            return (
              <button
                key={category}
                onClick={() => setSelectedCategory(category)}
                className={`rounded-full border px-3 py-1.5 text-[11px] font-bold uppercase tracking-wider transition-colors ${
                  active
                    ? "border-indigo-200 bg-indigo-50 text-indigo-700"
                    : "border-slate-200 bg-white text-slate-500 hover:border-slate-300 hover:text-slate-700"
                }`}
              >
                {category} ({count})
              </button>
            );
          })}
        </div>

        {filteredAuditEvents.length === 0 ? (
          <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
            No diagnostics recorded for the selected filter.
          </div>
        ) : (
          <div className="mt-6 space-y-3">
            {filteredAuditEvents.slice(0, 20).map((event, index) => (
              <DiagnosticEventCard
                key={`${event.recordedAtMs}-${event.category}-${index}`}
                event={event}
              />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function DiagnosticEventCard({ event }: { event: AuditEvent }) {
  const Icon = categoryIcon(event.category);

  return (
    <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4">
      <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span
              className={`inline-flex items-center gap-2 rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${diagnosticTone(event.category)}`}
            >
              <Icon className="h-3.5 w-3.5" />
              {event.category}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
              {event.kind}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-500">
              {new Date(event.recordedAtMs).toLocaleString()}
            </span>
          </div>
          <div className="mt-3 text-sm font-semibold text-slate-900">
            {event.title}
          </div>
          <div className="mt-2 whitespace-pre-wrap break-words rounded-xl border border-slate-200 bg-white px-4 py-3 font-mono text-xs leading-6 text-slate-600">
            {event.detail}
          </div>
        </div>
        <div className="grid grid-cols-1 gap-2 text-xs xl:w-56">
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">session_id</div>
            <div className="mt-1 font-medium text-slate-900">
              {event.sessionId || "n/a"}
            </div>
          </div>
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">pid</div>
            <div className="mt-1 font-medium text-slate-900">
              {event.pid ?? "n/a"}
            </div>
          </div>
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">runtime_id</div>
            <div className="mt-1 font-medium text-slate-900">
              {event.runtimeId || "n/a"}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export function ResourcesPage() {
  return <ControlCenterPage />;
}
