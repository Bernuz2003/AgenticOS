import { useEffect, useState } from "react";
import { CheckCircle2, LoaderCircle, Power } from "lucide-react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  listModels,
  loadModel,
  shutdownKernel,
  type ModelCatalogSnapshot,
} from "../lib/api";
import { NewAgentCard } from "../components/lobby/new-agent-card";
import { SessionCard } from "../components/lobby/session-card";
import { useSessionsStore } from "../store/sessions-store";

export function LobbyPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const sessions = useSessionsStore((state) => state.sessions);
  const orchestrations = useSessionsStore((state) => state.orchestrations);
  const connected = useSessionsStore((state) => state.connected);
  const selectedModelId = useSessionsStore((state) => state.selectedModelId);
  const loadedModelId = useSessionsStore((state) => state.loadedModelId);
  const loading = useSessionsStore((state) => state.loading);
  const error = useSessionsStore((state) => state.error);
  const refresh = useSessionsStore((state) => state.refresh);
  const applySnapshot = useSessionsStore((state) => state.applySnapshot);
  const setBridgeStatus = useSessionsStore((state) => state.setBridgeStatus);
  const [catalog, setCatalog] = useState<ModelCatalogSnapshot | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [selectedDraft, setSelectedDraft] = useState("");
  const [actionLoading, setActionLoading] = useState<"load" | "shutdown" | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

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
            La Lobby sostituisce il vecchio pannello processi: ogni card rappresenta una sessione osservabile, con PID runtime, strategy context e segnali essenziali di uptime e token usage.
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
                    {loadedModelId === model.id ? " · loaded" : ""}
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
              Loading model... il kernel continua a lavorare finche' non arriva la risposta reale.
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
                  {model.selected || loadedModelId === model.id ? (
                    <span className="status-pill border-emerald-600/20 bg-emerald-50 text-emerald-700">
                      <CheckCircle2 className="h-3.5 w-3.5" />
                      {loadedModelId === model.id ? "loaded" : "selected"}
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
