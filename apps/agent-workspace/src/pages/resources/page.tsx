import { useEffect, useMemo, useState } from "react";
import { ArrowRight } from "lucide-react";
import { Link } from "react-router-dom";

import { fetchOrchestrationStatus, type OrchestrationStatus } from "../../lib/api";
import { friendlyModelLabel, friendlyRuntimeLabel } from "../../lib/models/labels";
import {
  runtimeStateLabel,
  runtimeStateTone,
  statusTone,
  strategyLabel,
} from "../../lib/utils/formatting";
import { useSessionsStore } from "../../store/sessions-store";
import { DiagnosticsSection } from "./diagnostics-section";
import { RuntimeSection } from "./runtime-section";
import { WorkflowHealthSection } from "./workflow-health-section";
import { buildCategoryCounts } from "./format";

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
    globalAuditEvents,
    scheduledJobs,
    refresh,
  } = useSessionsStore();
  const [workflowDetails, setWorkflowDetails] = useState<Record<number, OrchestrationStatus>>(
    {},
  );
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
  const runningOrchestrations = orchestrations.filter((workflow) => !workflow.finished);
  const failedOrchestrations = orchestrations.filter((workflow) => workflow.failed > 0);
  const activeScheduledJobs = scheduledJobs.filter(
    (job) => job.state === "running" || job.state === "retry_wait",
  );
  const categoryCounts = useMemo(
    () => buildCategoryCounts(globalAuditEvents),
    [globalAuditEvents],
  );
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
              Interactive chat sessions stay visible with both UI status and live runtime
              state, so parked, idle and active processes are not conflated.
            </p>
          </div>
          <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
            {sessions.length} sessions
          </div>
        </div>

        {sessions.length === 0 ? (
          <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center">
            <div className="text-lg font-semibold text-slate-800">No sessions yet</div>
            <div className="mt-2 text-sm text-slate-500">
              Start a chat to populate the process board. Workflow task execution is monitored
              from Jobs and workflow run detail.
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
                        className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${statusTone(
                          session.status,
                        )}`}
                      >
                        {session.status}
                      </span>
                      <span
                        className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${runtimeStateTone(
                          session.runtimeState,
                        )}`}
                      >
                        {runtimeStateLabel(session.runtimeState)}
                      </span>
                    </div>
                    <div className="mt-1 text-xs text-slate-500">
                      session_id: {session.sessionId}
                    </div>
                    <div className="mt-3 text-sm text-slate-600">{session.promptPreview}</div>
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
                    {session.orchestrationTaskId ? ` · task ${session.orchestrationTaskId}` : ""}
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </section>

      <WorkflowHealthSection
        orchestrations={orchestrations}
        workflowDetails={workflowDetails}
        workflowError={workflowError}
      />

      <DiagnosticsSection
        events={filteredAuditEvents}
        categoryOptions={categoryOptions}
        categoryCounts={categoryCounts}
        diagnosticHighlights={diagnosticHighlights}
        selectedCategory={selectedCategory}
        onCategoryChange={setSelectedCategory}
      />
    </div>
  );
}

export function ResourcesPage() {
  return <ControlCenterPage />;
}
