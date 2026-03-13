import { useEffect, useState } from "react";
import { CheckCircle2, LoaderCircle, Power, Cloud } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  listModels,
  loadModel,
  shutdownKernel,
  type AuditEvent,
  type BackendCapabilities,
  type BackendTelemetry,
  type ModelCatalogSnapshot,
  type RemoteRuntimeModel,
} from "../lib/api";
import {
  isLoadedLocalCatalogModel,
  matchesLoadedRemoteTarget,
  selectRemoteCatalogTarget,
} from "../lib/remote-catalog";
import { NewAgentCard } from "../components/lobby/new-agent-card";
import { SessionCard } from "../components/lobby/session-card";
import { useSessionsStore } from "../store/sessions-store";

const CLOUD_BACKEND_STORAGE_KEY = "agenticos.cloudBackendId";
const CLOUD_MODEL_STORAGE_KEY = "agenticos.cloudModelId";

function loadStoredDraft(key: string, fallback: string): string {
  if (typeof window === "undefined") {
    return fallback;
  }

  const value = window.localStorage.getItem(key)?.trim();
  return value ? value : fallback;
}

function summarizeBackendCapabilities(capabilities: BackendCapabilities | null): string {
  if (!capabilities) {
    return "capabilities n/a";
  }

  const enabled: string[] = [];
  if (capabilities.streamingGeneration) {
    enabled.push("streaming");
  }
  if (capabilities.structuredOutput) {
    enabled.push("structured-output");
  }
  if (capabilities.toolPauseResume) {
    enabled.push("tool-pause");
  }
  if (capabilities.memoryTelemetry) {
    enabled.push("memory-telemetry");
  }

  return enabled.length > 0 ? enabled.join(" · ") : "no advanced capabilities";
}

function summarizeBackendTelemetry(telemetry: BackendTelemetry | null): string {
  if (!telemetry) {
    return "telemetry n/a";
  }

  return [
    `${telemetry.requestsTotal} req`,
    `${telemetry.inputTokensTotal}/${telemetry.outputTokensTotal} tok`,
    `$${telemetry.estimatedCostUsd.toFixed(6)}`,
  ].join(" · ");
}

function summarizeRemoteRuntimeModel(model: RemoteRuntimeModel | null): string {
  if (!model) {
    return "remote model n/a";
  }

  return [
    `${model.providerLabel} · ${model.modelLabel}`,
    model.contextWindowTokens ? `${model.contextWindowTokens.toLocaleString()} ctx` : "ctx n/a",
    model.maxOutputTokens ? `${model.maxOutputTokens.toLocaleString()} max out` : "out n/a",
    model.supportsStructuredOutput ? "structured output" : "plain output",
  ].join(" · ");
}

function summarizeCloudPricing(model: RemoteRuntimeModel | null): string {
  if (!model) {
    return "pricing n/a";
  }

  const input =
    model.inputPriceUsdPerMtok === null ? "in n/a" : `in $${model.inputPriceUsdPerMtok}/Mt`;
  const output =
    model.outputPriceUsdPerMtok === null ? "out n/a" : `out $${model.outputPriceUsdPerMtok}/Mt`;
  return `${input} · ${output}`;
}

function formatBytes(value: number | null | undefined): string {
  if (value === null || value === undefined) {
    return "n/a";
  }

  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let scaled = value;
  let unitIndex = 0;
  while (scaled >= 1024 && unitIndex < units.length - 1) {
    scaled /= 1024;
    unitIndex += 1;
  }

  return `${scaled.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function formatAuditTime(recordedAtMs: number): string {
  if (recordedAtMs <= 0) {
    return "replay";
  }

  return new Date(recordedAtMs).toLocaleTimeString("it-IT", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function LobbyPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const sessions = useSessionsStore((state) => state.sessions);
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const connected = useSessionsStore((state) => state.connected);
  const selectedModelId = useSessionsStore((state) => state.selectedModelId);
  const loadedModelId = useSessionsStore((state) => state.loadedModelId);
  const loadedTargetKind = useSessionsStore((state) => state.loadedTargetKind);
  const loadedProviderId = useSessionsStore((state) => state.loadedProviderId);
  const loadedRemoteModelId = useSessionsStore((state) => state.loadedRemoteModelId);
  const loadedBackendId = useSessionsStore((state) => state.loadedBackendId);
  const loadedBackendClass = useSessionsStore((state) => state.loadedBackendClass);
  const loadedBackendCapabilities = useSessionsStore(
    (state) => state.loadedBackendCapabilities,
  );
  const globalAccounting = useSessionsStore((state) => state.globalAccounting);
  const loadedBackendTelemetry = useSessionsStore(
    (state) => state.loadedBackendTelemetry,
  );
  const loadedRemoteModel = useSessionsStore((state) => state.loadedRemoteModel);
  const memory = useSessionsStore((state) => state.memory);
  const runtimeInstances = useSessionsStore((state) => state.runtimeInstances);
  const resourceGovernor = useSessionsStore((state) => state.resourceGovernor);
  const runtimeLoadQueue = useSessionsStore((state) => state.runtimeLoadQueue);
  const globalAuditEvents = useSessionsStore((state) => state.globalAuditEvents);
  const loading = useSessionsStore((state) => state.loading);
  const error = useSessionsStore((state) => state.error);
  const refresh = useSessionsStore((state) => state.refresh);
  const applySnapshot = useSessionsStore((state) => state.applySnapshot);
  const setBridgeStatus = useSessionsStore((state) => state.setBridgeStatus);
  const [catalog, setCatalog] = useState<ModelCatalogSnapshot | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [selectedDraft, setSelectedDraft] = useState("");
  const [cloudBackendDraft, setCloudBackendDraft] = useState(() =>
    loadStoredDraft(CLOUD_BACKEND_STORAGE_KEY, "openai-responses"),
  );
  const [cloudModelDraft, setCloudModelDraft] = useState(() =>
    loadStoredDraft(CLOUD_MODEL_STORAGE_KEY, "gpt-4.1-mini"),
  );
  const [actionLoading, setActionLoading] = useState<"load" | "shutdown" | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const remoteProviders = catalog?.remoteProviders ?? [];
  const {
    provider: selectedCloudBackend,
    model: selectedCloudModel,
    selector: cloudSelector,
  } = selectRemoteCatalogTarget(remoteProviders, cloudBackendDraft, cloudModelDraft);
  const loadedSelectedCloudTarget = matchesLoadedRemoteTarget(
    loadedTargetKind,
    loadedProviderId,
    loadedRemoteModelId,
    selectedCloudBackend?.id,
    selectedCloudModel?.id,
  );

  async function refreshCatalog() {
    setCatalogLoading(true);
    setCatalogError(null);
    try {
      const snapshot = await listModels();
      setCatalog(snapshot);
      setSelectedDraft((current) => {
        if (current && snapshot.models.some((model) => model.id === current)) {
          return current;
        }
        return snapshot.selectedModelId ?? snapshot.models[0]?.id ?? "";
      });
    } catch (catalogFetchError) {
      setCatalogError(
        catalogFetchError instanceof Error
          ? catalogFetchError.message
          : "Failed to load model catalog",
      );
    } finally {
      setCatalogLoading(false);
    }
  }

  useEffect(() => {
    void refreshCatalog();
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    window.localStorage.setItem(CLOUD_BACKEND_STORAGE_KEY, cloudBackendDraft);
  }, [cloudBackendDraft]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    window.localStorage.setItem(CLOUD_MODEL_STORAGE_KEY, cloudModelDraft);
  }, [cloudModelDraft]);

  useEffect(() => {
    if (remoteProviders.length === 0) {
      return;
    }
    if (!selectedCloudBackend) {
      setCloudBackendDraft(remoteProviders[0].id);
    }
  }, [remoteProviders, selectedCloudBackend]);

  useEffect(() => {
    if (!selectedCloudBackend) {
      if (cloudModelDraft) {
        setCloudModelDraft("");
      }
      return;
    }

    const availableModels = selectedCloudBackend.models;
    const fallbackModelId =
      availableModels.find((model) => model.id === selectedCloudBackend.defaultModelId)?.id ??
      availableModels[0]?.id ??
      "";

    if (!availableModels.some((model) => model.id === cloudModelDraft)) {
      setCloudModelDraft(fallbackModelId);
    }
  }, [selectedCloudBackend, cloudModelDraft]);

  useEffect(() => {
    const state = location.state as { focusComposer?: boolean } | null;
    if (!state?.focusComposer) {
      return;
    }

    const frame = window.requestAnimationFrame(() => {
      const card = document.getElementById("new-agent-card");
      const prompt = document.getElementById(
        "new-agent-prompt",
      ) as HTMLTextAreaElement | null;

      card?.scrollIntoView({ behavior: "smooth", block: "center" });
      prompt?.focus();
    });

    navigate(".", { replace: true, state: null });
    return () => window.cancelAnimationFrame(frame);
  }, [location.state, navigate]);

  async function syncLobbyAndCatalog() {
    await Promise.all([refresh(), refreshCatalog()]);
  }

  async function handleLoadModel() {
    if (!selectedDraft) {
      setActionError("Seleziona un modello dal catalogo.");
      return;
    }

    setActionLoading("load");
    setActionError(null);
    setActionMessage(null);
    try {
      const result = await loadModel(selectedDraft);
      setActionMessage(
        `Modello caricato: ${result.family} (${result.architecture || "arch n/a"}) via ${result.backend} [${result.loadMode}]`,
      );
      await syncLobbyAndCatalog();
    } catch (loadError) {
      setActionMessage(null);
      setActionError(
        loadError instanceof Error ? loadError.message : "Failed to load model",
      );
    } finally {
      setActionLoading(null);
    }
  }

  async function handleLoadCloudTarget() {
    if (!selectedCloudBackend) {
      setActionError("Nessun provider cloud configurato.");
      return;
    }

    setActionLoading("load");
    setActionError(null);
    setActionMessage(null);
    try {
      const result = await loadModel(cloudSelector);
      setActionMessage(
        `Target cloud caricato: ${cloudSelector} via ${result.backend} [${result.backendClass}]${result.remoteModel ? ` · ${result.remoteModel.modelLabel}` : ""}`,
      );
      await syncLobbyAndCatalog();
    } catch (loadError) {
      setActionMessage(null);
      setActionError(
        loadError instanceof Error ? loadError.message : "Failed to load cloud target",
      );
    } finally {
      setActionLoading(null);
    }
  }

  async function handleShutdownKernel() {
    setActionLoading("shutdown");
    setActionError(null);
    setActionMessage(null);
    try {
      const message = await shutdownKernel();
      setActionMessage(message);
      applySnapshot({
        connected: false,
        selectedModelId: "",
        loadedModelId: "",
        loadedTargetKind: null,
        loadedProviderId: null,
        loadedRemoteModelId: null,
        loadedBackendId: null,
        loadedBackendClass: null,
        loadedBackendCapabilities: null,
        globalAccounting: null,
        loadedBackendTelemetry: null,
        loadedRemoteModel: null,
        memory: null,
        runtimeInstances: [],
        resourceGovernor: null,
        runtimeLoadQueue: [],
        globalAuditEvents: [],
        orchestrations: [],
        sessions: [],
        error: null,
      });
      setBridgeStatus(false, null);
      setCatalog(null);
      setCatalogError(null);
      setSelectedDraft("");
    } catch (shutdownError) {
      setActionError(
        shutdownError instanceof Error
          ? shutdownError.message
          : "Failed to shutdown kernel",
      );
      await refresh();
    } finally {
      setActionLoading(null);
    }
  }

  return (
    <section className="space-y-6">
      <div className="grid gap-4 md:grid-cols-[1.2fr_0.8fr]">
        <div className="panel-surface px-6 py-5">
          <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
            Session Hub
          </p>
          <h2 className="mt-3 text-3xl font-bold tracking-tight text-slate-950">
            Lobby delle sessioni AgenticOS
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-6 text-slate-600">
            La Lobby sostituisce il vecchio pannello processi: ogni card rappresenta una sessione logica persistita, separata dal PID attivo e dal runtime target assegnato.
          </p>
        </div>
        <div className="panel-surface px-6 py-5">
          <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
            Control plane
          </p>
          <dl className="mt-4 space-y-3 text-sm text-slate-700">
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Bridge Rust</dt>
              <dd className="font-semibold text-slate-950">{connected ? "connected" : "disconnected"}</dd>
            </div>
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Selected model</dt>
              <dd className="font-semibold text-slate-950">{selectedModelId || "—"}</dd>
            </div>
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Loaded model</dt>
              <dd className="font-semibold text-slate-950">{loadedModelId || "—"}</dd>
            </div>
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Loaded target</dt>
              <dd className="font-semibold text-slate-950">{loadedTargetKind || "—"}</dd>
            </div>
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Loaded provider</dt>
              <dd className="font-semibold text-slate-950">
                {loadedProviderId || "—"}
                {loadedRemoteModelId ? ` · ${loadedRemoteModelId}` : ""}
              </dd>
            </div>
            <div className="flex items-center justify-between rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt>Loaded backend</dt>
              <dd className="font-semibold text-slate-950">
                {loadedBackendId || "—"}
                {loadedBackendClass ? ` · ${loadedBackendClass}` : ""}
              </dd>
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Backend caps</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {summarizeBackendCapabilities(loadedBackendCapabilities)}
              </dd>
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Global accounting</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {summarizeBackendTelemetry(globalAccounting)}
              </dd>
              {globalAccounting?.lastError ? (
                <div className="mt-2 text-xs leading-5 text-rose-700">
                  last error: {globalAccounting.lastError}
                </div>
              ) : null}
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Loaded backend history</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {summarizeBackendTelemetry(loadedBackendTelemetry)}
              </dd>
              {loadedBackendTelemetry?.lastError ? (
                <div className="mt-2 text-xs leading-5 text-rose-700">
                  last error: {loadedBackendTelemetry.lastError}
                </div>
              ) : null}
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Loaded remote model</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {summarizeRemoteRuntimeModel(loadedRemoteModel)}
              </dd>
              <div className="mt-2 text-xs leading-5 text-slate-600">
                {summarizeCloudPricing(loadedRemoteModel)}
                {loadedRemoteModel ? ` · adapter ${loadedRemoteModel.adapterKind}` : ""}
              </div>
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Memory / swap</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {memory
                  ? `alloc ${formatBytes(memory.allocBytes)} · evict ${memory.evictions} · swap ${memory.swapCount}/${memory.swapFaults}`
                  : "memory snapshot n/a"}
              </dd>
              {memory ? (
                <div className="mt-2 text-xs leading-5 text-slate-600">
                  active={String(memory.active)} · blocks {memory.freeBlocks}/{memory.totalBlocks} · tracked_pids {memory.trackedPids} · oom {memory.oomEvents}
                </div>
              ) : null}
            </div>
            <div className="rounded-2xl bg-slate-950/[0.04] px-4 py-3">
              <dt className="text-slate-700">Resource governor</dt>
              <dd className="mt-2 text-xs font-semibold uppercase tracking-[0.16em] text-slate-500">
                {resourceGovernor
                  ? `ram ${formatBytes(resourceGovernor.ramUsedBytes)}/${formatBytes(resourceGovernor.ramBudgetBytes)} · vram ${formatBytes(resourceGovernor.vramUsedBytes)}/${formatBytes(resourceGovernor.vramBudgetBytes)}`
                  : "governor n/a"}
              </dd>
              {resourceGovernor ? (
                <div className="mt-2 text-xs leading-5 text-slate-600">
                  headroom ram {formatBytes(resourceGovernor.minRamHeadroomBytes)} · headroom vram {formatBytes(resourceGovernor.minVramHeadroomBytes)}
                  <br />
                  pending queue {resourceGovernor.pendingQueueDepth} · loader {resourceGovernor.loaderBusy ? `busy (${resourceGovernor.loaderReason || "n/a"})` : "idle"}
                </div>
              ) : null}
            </div>
          </dl>
          <div className="mt-4 flex items-center gap-3">
            <button
              onClick={() => void refresh()}
              className="rounded-full border border-slate-900/10 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:border-slate-900/20 hover:text-slate-950"
            >
              {loading ? "Aggiornamento..." : "Refresh Lobby"}
            </button>
            <button
              onClick={() => void refreshCatalog()}
              className="rounded-full border border-slate-900/10 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:border-slate-900/20 hover:text-slate-950"
            >
              {catalogLoading ? "Catalogo..." : "Refresh Catalog"}
            </button>
            {error ? <span className="text-xs text-rose-700">{error}</span> : null}
          </div>
        </div>
      </div>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)]">
        <section className="panel-surface p-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
                Runtime inventory
              </p>
              <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
                Runtime registrati nel kernel
              </h3>
            </div>
            <span className="status-pill border-slate-900/10 bg-slate-100 text-slate-700">
              {runtimeInstances.length} runtimes
            </span>
          </div>
          <div className="mt-5 grid gap-3 xl:grid-cols-2">
            {runtimeInstances.map((runtime) => (
              <article key={runtime.runtimeId} className="rounded-[24px] border border-slate-200 bg-white p-4">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
                      {runtime.runtimeId}
                    </div>
                    <div className="mt-1 text-base font-semibold text-slate-950">
                      {runtime.logicalModelId}
                    </div>
                  </div>
                  <span className="status-pill border-slate-900/10 bg-slate-100 text-slate-700">
                    {runtime.state}
                  </span>
                </div>
                <div className="mt-3 text-sm leading-6 text-slate-600">
                  backend={runtime.backendId} · class={runtime.backendClass} · family={runtime.family}
                  <br />
                  target={runtime.targetKind} · provider={runtime.providerId || "n/a"} · remote_model={runtime.remoteModelId || "n/a"}
                  <br />
                  ram={formatBytes(runtime.reservationRamBytes)} · vram={formatBytes(runtime.reservationVramBytes)} · active_pids={runtime.activePids.length ? runtime.activePids.join(", ") : "none"}
                  <br />
                  pinned={String(runtime.pinned)} · transition={runtime.transitionState || "steady"} · current={String(runtime.current)}
                </div>
              </article>
            ))}
            {runtimeInstances.length === 0 ? (
              <div className="rounded-[24px] border border-dashed border-slate-300 px-4 py-8 text-sm text-slate-500">
                Nessun runtime persistito o attivo disponibile.
              </div>
            ) : null}
          </div>
        </section>

        <section className="panel-surface p-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
                Runtime queue
              </p>
              <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
                Admission e richieste pendenti
              </h3>
            </div>
            <span className="status-pill border-slate-900/10 bg-slate-100 text-slate-700">
              {runtimeLoadQueue.length} entries
            </span>
          </div>
          <div className="mt-5 space-y-3">
            {runtimeLoadQueue.map((entry) => (
              <article key={entry.queueId} className="rounded-[24px] bg-slate-950/[0.04] p-4">
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-semibold text-slate-950">
                    {entry.logicalModelId}
                  </div>
                  <span className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
                    {entry.state}
                  </span>
                </div>
                <div className="mt-2 text-xs leading-5 text-slate-600">
                  backend_class={entry.backendClass} · ram={formatBytes(entry.reservationRamBytes)} · vram={formatBytes(entry.reservationVramBytes)}
                  <br />
                  {entry.reason}
                </div>
              </article>
            ))}
            {runtimeLoadQueue.length === 0 ? (
              <div className="rounded-[24px] border border-dashed border-slate-300 px-4 py-8 text-sm text-slate-500">
                Nessuna richiesta di load accodata.
              </div>
            ) : null}
          </div>
        </section>
      </div>

      <section className="panel-surface px-6 py-5">
        <div className="flex items-center justify-between gap-3">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
              Global audit
            </p>
            <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
              Timeline tecnica del kernel
            </h3>
          </div>
          <span className="status-pill border-slate-900/10 bg-slate-100 text-slate-700">
            {globalAuditEvents.length} recent
          </span>
        </div>
        <div className="mt-5 grid gap-3 lg:grid-cols-2">
          {globalAuditEvents.map((event: AuditEvent, index) => (
            <article
              key={`${event.recordedAtMs}-${event.kind}-${index}`}
              className="rounded-[24px] border border-slate-200 bg-white p-4"
            >
              <div className="flex items-center justify-between gap-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-500">
                <span>
                  {event.category} · {event.kind}
                </span>
                <span>{formatAuditTime(event.recordedAtMs)}</span>
              </div>
              <div className="mt-2 text-base font-semibold text-slate-950">
                {event.title}
              </div>
              <div className="mt-2 font-mono text-xs leading-5 text-slate-600">
                {event.detail}
              </div>
              <div className="mt-3 text-[11px] text-slate-500">
                {event.sessionId ? `session ${event.sessionId}` : "global"}
                {event.pid !== null ? ` · pid ${event.pid}` : ""}
                {event.runtimeId ? ` · runtime ${event.runtimeId}` : ""}
              </div>
            </article>
          ))}
          {globalAuditEvents.length === 0 ? (
            <div className="rounded-[24px] border border-dashed border-slate-300 px-4 py-8 text-sm text-slate-500">
              Nessun evento audit persistito disponibile.
            </div>
          ) : null}
        </div>
      </section>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.35fr)_minmax(0,0.65fr)]">
        <section className="panel-surface p-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
                Models
              </p>
              <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
                Catalogo e load control
              </h3>
            </div>
            <span className="status-pill border-slate-900/10 bg-slate-100 text-slate-700">
              {catalog?.totalModels ?? 0} models
            </span>
          </div>

          <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_auto]">
            <div className="space-y-2">
              <label
                htmlFor="model-select"
                className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500"
              >
                Model selection
              </label>
              <select
                id="model-select"
                value={selectedDraft}
                onChange={(event) => setSelectedDraft(event.target.value)}
                className="w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-900 outline-none transition focus:border-slate-400"
                disabled={catalogLoading || !catalog?.models.length}
              >
                <option value="">Seleziona un modello</option>
                {catalog?.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.id}
                    {model.selected ? " · selected" : ""}
                    {isLoadedLocalCatalogModel(loadedTargetKind, loadedModelId, model.id)
                      ? " · loaded"
                      : ""}
                  </option>
                ))}
              </select>
            </div>

            <div className="flex flex-wrap items-end gap-3">
              <button
                onClick={() => void handleLoadModel()}
                disabled={actionLoading !== null || !selectedDraft}
                className="rounded-full bg-slate-950 px-4 py-2 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {actionLoading === "load" ? "Loading..." : "Load"}
              </button>
              <button
                onClick={() => void handleShutdownKernel()}
                disabled={actionLoading !== null || !connected}
                className="inline-flex items-center gap-2 rounded-full border border-rose-200 bg-rose-50 px-4 py-2 text-sm font-semibold text-rose-700 transition hover:bg-rose-100 disabled:cursor-not-allowed disabled:opacity-60"
              >
                <Power className="h-4 w-4" />
                {actionLoading === "shutdown" ? "Stopping..." : "Shutdown"}
              </button>
            </div>
          </div>

          {actionMessage ? (
            <div className="mt-4 rounded-2xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-800">
              {actionMessage}
            </div>
          ) : null}
          {actionLoading === "load" ? (
            <div className="mt-4 rounded-2xl border border-sky-200 bg-sky-50 px-4 py-3 text-sm text-sky-900">
              Loading model...
            </div>
          ) : null}
          {actionError ? (
            <div className="mt-4 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
              {actionError}
            </div>
          ) : null}
          {catalogError ? (
            <div className="mt-4 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800">
              {catalogError}
            </div>
          ) : null}

          <div className="mt-5 grid gap-3 xl:grid-cols-2">
            {(catalog?.models ?? []).slice(0, 6).map((model) => (
              <article key={model.id} className="rounded-[24px] border border-slate-200 bg-white p-4">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
                      {model.family}
                    </div>
                    <div className="mt-1 text-base font-semibold text-slate-950">
                      {model.id}
                    </div>
                  </div>
                  {model.selected ||
                  isLoadedLocalCatalogModel(loadedTargetKind, loadedModelId, model.id) ? (
                    <span className="status-pill border-emerald-600/20 bg-emerald-50 text-emerald-700">
                      <CheckCircle2 className="h-3.5 w-3.5" />
                      {isLoadedLocalCatalogModel(loadedTargetKind, loadedModelId, model.id)
                        ? "loaded"
                        : "selected"}
                    </span>
                  ) : null}
                </div>
                <div className="mt-3 text-sm text-slate-600">
                  arch {model.architecture || "n/a"} · backend {model.resolvedBackend || "n/a"} · source {model.driverResolutionSource}
                </div>
                <div className="mt-2 text-xs leading-5 text-slate-500">
                  {model.driverResolutionRationale}
                </div>
              </article>
            ))}
            {!catalogLoading && (catalog?.models.length ?? 0) === 0 ? (
              <div className="rounded-[24px] border border-dashed border-slate-300 px-4 py-8 text-sm text-slate-500">
                Nessun modello scoperto dal catalogo.
              </div>
            ) : null}
          </div>
        </section>

        <section className="panel-surface p-6">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
                Routing
              </p>
              <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
                Workload picks
              </h3>
            </div>
            {catalogLoading ? <LoaderCircle className="h-4 w-4 animate-spin text-slate-500" /> : null}
          </div>
          <div className="mt-5 space-y-3">
            {(catalog?.routingRecommendations ?? []).map((recommendation) => (
              <article key={recommendation.workload} className="rounded-[24px] bg-slate-950/[0.04] p-4">
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-semibold uppercase tracking-[0.18em] text-slate-500">
                    {recommendation.workload}
                  </div>
                  <span className="text-xs font-semibold text-slate-700">
                    {recommendation.source}
                  </span>
                </div>
                <div className="mt-2 text-base font-semibold text-slate-950">
                  {recommendation.modelId || "No recommended model"}
                </div>
                <div className="mt-2 text-sm leading-6 text-slate-600">
                  {recommendation.rationale}
                </div>
              </article>
            ))}
            {catalog?.routingRecommendations.length ? null : (
              <div className="rounded-[24px] border border-dashed border-slate-300 px-4 py-8 text-sm text-slate-500">
                Nessuna raccomandazione disponibile.
              </div>
            )}
          </div>
        </section>
      </div>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
        <section className="panel-surface p-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
                Cloud runtime
              </p>
              <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
                Test target remoto
              </h3>
            </div>
            <span className="status-pill border-sky-700/15 bg-sky-50 text-sky-700">
              <Cloud className="h-3.5 w-3.5" />
              remote_stateless
            </span>
          </div>

          <p className="mt-3 max-w-3xl text-sm leading-6 text-slate-600">
            Questo pannello costruisce un selector esplicito `cloud:&lt;backend&gt;:&lt;model&gt;`
            e invia `LOAD` al kernel. Endpoint, API key e default model restano configurati lato
            kernel tramite `config/kernel/base.toml`, `config/kernel/local.toml` oppure
            environment variables caricate da `config/env/agenticos.env`.
          </p>

          <div className="mt-5 grid gap-4 lg:grid-cols-[0.42fr_0.58fr_auto]">
            <div className="space-y-2">
              <label
                htmlFor="cloud-backend"
                className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500"
              >
                Backend
              </label>
              <select
                id="cloud-backend"
                value={selectedCloudBackend?.id ?? ""}
                onChange={(event) => setCloudBackendDraft(event.target.value)}
                className="w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-900 outline-none transition focus:border-slate-400"
                disabled={actionLoading !== null || remoteProviders.length === 0}
              >
                {remoteProviders.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <label
                htmlFor="cloud-model"
                className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500"
              >
                Remote model
              </label>
              <select
                id="cloud-model"
                value={cloudModelDraft}
                onChange={(event) => setCloudModelDraft(event.target.value)}
                className="w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-900 outline-none transition focus:border-slate-400"
                disabled={actionLoading !== null || !selectedCloudBackend}
              >
                {selectedCloudBackend?.models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="flex items-end">
              <button
                onClick={() => void handleLoadCloudTarget()}
                disabled={actionLoading !== null || !selectedCloudBackend}
                className="rounded-full bg-sky-700 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-800 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {actionLoading === "load" ? "Loading..." : "Load Cloud"}
              </button>
            </div>
          </div>

          <div className="mt-4 rounded-[24px] border border-slate-200 bg-slate-50 px-4 py-4">
            <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500">
              Selector preview
            </div>
            <div className="mt-2 font-mono text-sm text-slate-900">{cloudSelector}</div>
          </div>
          <div className="mt-4 rounded-[24px] border border-slate-200 bg-white px-4 py-4">
            <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-slate-500">
              Selected remote model
            </div>
            <div className="mt-2 text-sm font-semibold text-slate-950">
              {selectedCloudModel?.label ?? "Nessun modello remoto selezionato"}
            </div>
            <div className="mt-2 text-xs uppercase tracking-[0.16em] text-slate-500">
              {selectedCloudBackend
                ? `${selectedCloudBackend.label} · adapter ${selectedCloudBackend.adapterKind}`
                : "provider n/a"}
            </div>
            <div className="mt-2 text-xs leading-5 text-slate-600">
              {loadedSelectedCloudTarget
                ? "Attualmente caricato nel runtime."
                : loadedTargetKind === "remote_provider"
                  ? `Runtime attivo: ${loadedProviderId || "provider n/a"} · ${loadedRemoteModelId || "model n/a"}`
                  : "Nessun target cloud attualmente caricato."}
            </div>
            <div className="mt-3 grid gap-2 sm:grid-cols-2">
              <div className="rounded-2xl bg-slate-950/[0.04] px-3 py-2 text-sm text-slate-700">
                Context window:{" "}
                <span className="font-semibold text-slate-950">
                  {selectedCloudModel?.contextWindowTokens?.toLocaleString() ?? "n/a"}
                </span>
              </div>
              <div className="rounded-2xl bg-slate-950/[0.04] px-3 py-2 text-sm text-slate-700">
                Max output:{" "}
                <span className="font-semibold text-slate-950">
                  {selectedCloudModel?.maxOutputTokens?.toLocaleString() ?? "n/a"}
                </span>
              </div>
              <div className="rounded-2xl bg-slate-950/[0.04] px-3 py-2 text-sm text-slate-700">
                Structured output:{" "}
                <span className="font-semibold text-slate-950">
                  {selectedCloudModel?.supportsStructuredOutput ? "yes" : "no"}
                </span>
              </div>
              <div className="rounded-2xl bg-slate-950/[0.04] px-3 py-2 text-sm text-slate-700">
                Pricing:{" "}
                <span className="font-semibold text-slate-950">
                  {selectedCloudModel
                    ? `${selectedCloudModel.inputPriceUsdPerMtok ?? "n/a"} / ${selectedCloudModel.outputPriceUsdPerMtok ?? "n/a"}`
                    : "n/a"}
                </span>
              </div>
            </div>
          </div>
        </section>

        <section className="panel-surface p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
            Test notes
          </p>
          <h3 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
            Prima di provare
          </h3>
          <div className="mt-5 space-y-3 text-sm leading-6 text-slate-600">
            <div className="rounded-[24px] bg-slate-950/[0.04] p-4">
              Credenziali richieste:{" "}
              {selectedCloudBackend?.credentialHint ??
                "Configura le credenziali del provider nel kernel."}
            </div>
            <div className="rounded-[24px] bg-slate-950/[0.04] p-4">
              Se non passi un tokenizer hint locale, il kernel usera' il fallback tokenizer per il backend `remote_stateless`.
            </div>
            <div className="rounded-[24px] bg-slate-950/[0.04] p-4">
              Profilo selezionato:{" "}
              {selectedCloudBackend?.note ??
                "I provider remoti arrivano dal catalogo remoto del kernel."}
            </div>
            <div className="rounded-[24px] bg-slate-950/[0.04] p-4">
              Dopo il `LOAD` puoi aprire una nuova sessione come sempre e verificare dal control plane che il backend caricato sia `{selectedCloudBackend?.id ?? "n/a"}`.
            </div>
          </div>
        </section>
      </div>

      <div className="grid gap-5 xl:grid-cols-3">
        <NewAgentCard />
        {sessions.map((session) => (
          <SessionCard key={session.sessionId} session={session} />
        ))}
      </div>
      {orchestrations.length > 0 ? (
        <div className="space-y-3">
          <div className="px-1 text-xs font-semibold uppercase tracking-[0.24em] text-slate-500">
            Orchestrations
          </div>
          <div className="grid gap-4 xl:grid-cols-3">
            {orchestrations.map((orchestration) => (
              <article key={orchestration.orchestrationId} className="panel-surface p-5">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">
                      Orchestration {orchestration.orchestrationId}
                    </div>
                    <h3 className="mt-2 text-xl font-bold text-slate-950">
                      {orchestration.running} running · {orchestration.pending} pending
                    </h3>
                  </div>
                  <span className="status-pill border-cyan-700/15 bg-cyan-50 text-cyan-700">
                    {orchestration.policy}
                  </span>
                </div>
                <div className="mt-4 grid grid-cols-4 gap-2 text-sm text-slate-600">
                  <div className="rounded-2xl bg-slate-950/[0.04] p-3">
                    <div className="text-[11px] uppercase tracking-[0.18em] text-slate-500">Total</div>
                    <div className="mt-2 font-semibold text-slate-950">{orchestration.total}</div>
                  </div>
                  <div className="rounded-2xl bg-slate-950/[0.04] p-3">
                    <div className="text-[11px] uppercase tracking-[0.18em] text-slate-500">Done</div>
                    <div className="mt-2 font-semibold text-slate-950">{orchestration.completed}</div>
                  </div>
                  <div className="rounded-2xl bg-slate-950/[0.04] p-3">
                    <div className="text-[11px] uppercase tracking-[0.18em] text-slate-500">Failed</div>
                    <div className="mt-2 font-semibold text-slate-950">{orchestration.failed}</div>
                  </div>
                  <div className="rounded-2xl bg-slate-950/[0.04] p-3">
                    <div className="text-[11px] uppercase tracking-[0.18em] text-slate-500">Elapsed</div>
                    <div className="mt-2 font-semibold text-slate-950">{orchestration.elapsedLabel}</div>
                  </div>
                </div>
              </article>
            ))}
          </div>
        </div>
      ) : null}
      {!loading && sessions.length === 0 ? (
        <div className="panel-surface px-6 py-10 text-center text-sm text-slate-600">
          Nessuna sessione attiva rilevata dal kernel. La Lobby usa gia' lo snapshot reale di `STATUS` e non dipende piu' da seed statici.
        </div>
      ) : null}
    </section>
  );
}
