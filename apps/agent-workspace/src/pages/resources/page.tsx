import { Link } from "react-router-dom";

import { friendlyModelLabel } from "../../lib/models/labels";
import { useSessionsStore } from "../../store/sessions-store";
import { McpSection } from "./mcp-section";
import { RuntimeSection } from "./runtime-section";

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
    managedLocalRuntimes,
    resourceGovernor,
    runtimeLoadQueue,
    mcp,
    scheduledJobs,
    refresh,
  } = useSessionsStore();

  const runningSessions = sessions.filter((session) => session.status === "running");
  const parkedSessions = sessions.filter((session) => session.status === "swapped");
  const idleSessions = sessions.filter((session) => session.status === "idle");
  const runningOrchestrations = orchestrations.filter((workflow) => !workflow.finished);
  const failedOrchestrations = orchestrations.filter((workflow) => workflow.failed > 0);
  const activeScheduledJobs = scheduledJobs.filter(
    (job) => job.state === "running" || job.state === "retry_wait",
  );

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
              A focused operator view for bridge health, loaded backend state,
              resident runtimes, memory pressure and runtime load queue.
              Conversation and orchestration details stay in their dedicated surfaces.
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
          <div className="mt-3 text-lg font-bold text-slate-900">{sessions.length} sessions</div>
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
            {runningOrchestrations.length} active workflows, {activeScheduledJobs.length} active
            scheduler jobs, {failedOrchestrations.length} workflows with failures.
          </div>
        </section>
      </div>

      <RuntimeSection
        loadedProviderId={loadedProviderId}
        loadedRemoteModelId={loadedRemoteModelId}
        loadedTargetKind={loadedTargetKind}
        loadedBackendId={loadedBackendId}
        loadedBackendClass={loadedBackendClass}
        loadedBackendCapabilities={loadedBackendCapabilities}
        loadedBackendTelemetry={loadedBackendTelemetry}
        loadedRemoteModel={loadedRemoteModel}
        globalAccounting={globalAccounting}
        managedLocalRuntimes={managedLocalRuntimes}
        runtimeInstances={runtimeInstances}
        resourceGovernor={resourceGovernor}
        memory={memory}
        runtimeLoadQueue={runtimeLoadQueue}
      />

      <McpSection mcp={mcp} />
    </div>
  );
}

export function ResourcesPage() {
  return <ControlCenterPage />;
}
