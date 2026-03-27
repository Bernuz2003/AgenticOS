import { Database, ShieldAlert } from "lucide-react";

import type {
  BackendCapabilities,
  BackendTelemetry,
  ManagedLocalRuntime,
  MemoryStatus,
  RemoteRuntimeModel,
  ResourceGovernorStatus,
  RuntimeInstance,
  RuntimeLoadQueueEntry,
} from "../../lib/api";
import { RuntimeHealthCard } from "../../components/diagnostics/runtime-health";
import { friendlyModelLabel } from "../../lib/models/labels";
import {
  localRuntimeStateLabel,
  localRuntimeStateTone,
} from "../../lib/utils/formatting";
import {
  formatBytes,
  formatPercent,
  formatRelative,
} from "./format";

interface RuntimeSectionProps {
  loadedProviderId: string | null;
  loadedRemoteModelId: string | null;
  loadedTargetKind: string | null;
  loadedBackendId: string | null;
  loadedBackendClass: string | null;
  loadedBackendCapabilities: BackendCapabilities | null;
  loadedBackendTelemetry: BackendTelemetry | null;
  loadedRemoteModel: RemoteRuntimeModel | null;
  globalAccounting: BackendTelemetry | null;
  managedLocalRuntimes: ManagedLocalRuntime[];
  runtimeInstances: RuntimeInstance[];
  resourceGovernor: ResourceGovernorStatus | null;
  memory: MemoryStatus | null;
  runtimeLoadQueue: RuntimeLoadQueueEntry[];
}

export function RuntimeSection({
  loadedProviderId,
  loadedRemoteModelId,
  loadedTargetKind,
  loadedBackendId,
  loadedBackendClass,
  loadedBackendCapabilities,
  loadedBackendTelemetry,
  loadedRemoteModel,
  globalAccounting,
  managedLocalRuntimes,
  runtimeInstances,
  resourceGovernor,
  memory,
  runtimeLoadQueue,
}: RuntimeSectionProps) {
  return (
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
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Local Runtime Manager
          </div>
          {managedLocalRuntimes.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-6 text-sm text-slate-500">
              No managed local family runtime is active yet.
            </div>
          ) : (
            managedLocalRuntimes.map((runtime) => (
              <RuntimeHealthCard
                key={`${runtime.family}:${runtime.logicalModelId}`}
                title={`${runtime.family} runtime`}
                subtitle={`${friendlyModelLabel(runtime.logicalModelId)} · port ${runtime.port}`}
                error={runtime.lastError}
                badges={
                  <>
                    <span
                      className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${localRuntimeStateTone(
                        runtime.state,
                      )}`}
                    >
                      {localRuntimeStateLabel(runtime.state)}
                    </span>
                    <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                      {runtime.managedByKernel ? "managed" : "external"}
                    </span>
                  </>
                }
                metrics={[
                  {
                    label: "Context",
                    value: runtime.contextWindowTokens
                      ? runtime.contextWindowTokens.toLocaleString()
                      : "n/a",
                  },
                  {
                    label: "Endpoint",
                    value: `127.0.0.1:${runtime.port}`,
                  },
                  {
                    label: "Slots",
                    value: runtime.slotSaveDir.split("/").slice(-2).join("/"),
                  },
                ]}
              />
            ))
          )}
        </div>

        <div className="mt-6 space-y-3">
          {runtimeInstances.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-center text-sm text-slate-500">
              No resident runtime is currently loaded.
            </div>
          ) : (
            runtimeInstances.map((runtime) => (
              <RuntimeHealthCard
                key={runtime.runtimeId}
                title={friendlyModelLabel(runtime.logicalModelId)}
                subtitle={`runtime_id: ${runtime.runtimeId} · backend: ${runtime.backendClass}`}
                badges={
                  <>
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
                  </>
                }
                metrics={[
                  { label: "Active PIDs", value: String(runtime.activePidCount) },
                  {
                    label: "VRAM",
                    value: formatBytes(runtime.reservationVramBytes),
                  },
                  {
                    label: "RAM",
                    value: formatBytes(runtime.reservationRamBytes),
                  },
                ]}
              />
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
                  <div className="mt-1 font-medium text-slate-900">{memory.trackedPids}</div>
                </div>
                <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
                  <div className="text-slate-500">Parked PIDs</div>
                  <div className="mt-1 font-medium text-slate-900">{memory.parkedPids}</div>
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
                  <div className="mt-1 font-medium text-amber-800">{memory.swapFaults}</div>
                </div>
                <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-3">
                  <div className="text-rose-600">OOM events</div>
                  <div className="mt-1 font-medium text-rose-800">{memory.oomEvents}</div>
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
              <RuntimeHealthCard
                key={entry.queueId}
                title={friendlyModelLabel(entry.logicalModelId)}
                subtitle={`${entry.backendClass} · ${entry.reason}`}
                metrics={[
                  { label: "State", value: entry.state },
                  { label: "Requested", value: formatRelative(entry.requestedAtMs) },
                  {
                    label: "VRAM",
                    value: formatBytes(entry.reservationVramBytes),
                  },
                ]}
              />
            ))
          )}
        </div>
      </section>
    </div>
  );
}
