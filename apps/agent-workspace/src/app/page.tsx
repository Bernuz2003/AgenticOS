import { useEffect, useState } from "react";
import { Info, Play, Server } from "lucide-react";
import {
  listModels,
  loadModel,
  type ModelCatalogSnapshot,
} from "../lib/api";
import { friendlyModelLabel } from "../lib/models/labels";
import { useSessionsStore } from "../store/sessions-store";

export function DashboardPage() {
  const [catalog, setCatalog] = useState<ModelCatalogSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<"local" | "remote">("local");

  const [selectedLocalId, setSelectedLocalId] = useState("");
  const [selectedCloudProviderId, setSelectedCloudProviderId] = useState("");
  const [selectedCloudModelId, setSelectedCloudModelId] = useState("");

  const refreshLobby = useSessionsStore((state) => state.refresh);
  const loadedModelId = useSessionsStore((state) => state.loadedModelId);
  const loadedBackendId = useSessionsStore((state) => state.loadedBackendId);
  const loadedTargetKind = useSessionsStore((state) => state.loadedTargetKind);

  useEffect(() => {
    void loadCatalog();
  }, []);

  async function loadCatalog() {
    try {
      setLoading(true);
      const data = await listModels();
      setCatalog(data);

      if (data.models.length > 0) {
        setSelectedLocalId(data.models[0].id);
      }
      if (data.remoteProviders.length > 0) {
        const prod = data.remoteProviders[0];
        setSelectedCloudProviderId(prod.id);
        if (prod.models.length > 0) {
          setSelectedCloudModelId(prod.models[0].id);
        }
      }
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }

  async function handleLoadLocal(path: string) {
    if (!path) return;
    try {
      setActionLoading(true);
      await loadModel(path);
      await refreshLobby();
    } catch (e) {
      console.error(e);
      alert(e);
    } finally {
      setActionLoading(false);
    }
  }

  async function handleLoadRemote() {
    if (!selectedCloudProviderId || !selectedCloudModelId) return;
    const provider = catalog?.remoteProviders.find(p => p.id === selectedCloudProviderId);
    if (!provider) return;

    const selector = `cloud:${provider.backendId}:${selectedCloudModelId}`;
    try {
      setActionLoading(true);
      await loadModel(selector);
      await refreshLobby();
    } catch (e) {
      console.error(e);
      alert(e);
    } finally {
      setActionLoading(false);
    }
  }

  const selectedLocalModel = catalog?.models.find((m) => m.id === selectedLocalId);
  const selectedCloudProvider = catalog?.remoteProviders.find((p) => p.id === selectedCloudProviderId);
  const selectedCloudModel = selectedCloudProvider?.models.find((m) => m.id === selectedCloudModelId);

  return (
    <div className="max-w-4xl mx-auto space-y-8">
      <section className="bg-indigo-900 rounded-3xl p-8 text-white shadow-sm relative overflow-hidden">
        <div className="absolute top-0 right-0 p-12 opacity-10">
          <Server className="w-48 h-48" />
        </div>
        <div className="relative z-10">
          <h1 className="text-3xl font-bold tracking-tight mb-2">Benvenuto nel Control Plane</h1>
          <p className="text-indigo-200 mb-8 max-w-xl">
            Questa e' la Dashboard di sistema di AgenticOS. Puoi ispezionare le risorse del kernel, avviare runtimes locali oppure connetterti ai provider cloud per gestire gli agenti strutturati in piena autonomia.
          </p>
          <div className="flex gap-4">
            <div className="bg-indigo-800/50 rounded-2xl px-5 py-4 border border-indigo-700/50 flex-1">
              <div className="text-indigo-300 text-xs uppercase tracking-wider font-semibold mb-1">Target Attivo</div>
              <div className="font-semibold">
                {loadedModelId
                  ? friendlyModelLabel(loadedModelId)
                  : "Nessun target attualmente in memoria"}
              </div>
              <div className="text-indigo-400 text-sm mt-1">{loadedTargetKind ? `${loadedTargetKind} via ${loadedBackendId}` : "Il resource governor non ha allocato memoria VRAM."}</div>
            </div>
            <div className="bg-indigo-800/50 rounded-2xl px-5 py-4 border border-indigo-700/50 flex-1">
              <div className="text-indigo-300 text-xs uppercase tracking-wider font-semibold mb-1">Stato Sistema</div>
              <div className="font-semibold flex items-center gap-2">
                <div className="w-2.5 h-2.5 rounded-full bg-emerald-400"></div> All Systems Operational
              </div>
            </div>
          </div>
        </div>
      </section>

      <section>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-xl font-bold text-slate-900">Load Runtime Target</h2>
          <div className="flex bg-slate-100 p-1 rounded-lg">
            <button
              onClick={() => setActiveTab("local")}
              className={`px-4 py-1.5 text-sm font-semibold rounded-md transition-shadow ${activeTab === "local" ? "bg-white text-slate-900 shadow-sm" : "text-slate-500 hover:text-slate-700"}`}
            >
              Locali (GGUF)
            </button>
            <button
              onClick={() => setActiveTab("remote")}
              className={`px-4 py-1.5 text-sm font-semibold rounded-md transition-shadow ${activeTab === "remote" ? "bg-white text-slate-900 shadow-sm" : "text-slate-500 hover:text-slate-700"}`}
            >
              Cloud Providers
            </button>
          </div>
        </div>

        {loading ? (
          <div className="h-48 flex items-center justify-center border-2 border-dashed border-slate-200 rounded-3xl">
            <span className="text-slate-400 font-medium">Loading catalog...</span>
          </div>
        ) : (
          <div className="bg-white border text-left border-slate-200 rounded-3xl p-6 shadow-sm">
            {activeTab === "local" && (
              <div className="space-y-6 flex flex-col items-start text-left">
                <div className="w-full">
                  <label className="block text-sm font-semibold text-slate-700 mb-2">Modello Locale (GGUF)</label>
                  <select
                    className="w-full bg-slate-50 border border-slate-200 rounded-xl px-4 py-3 text-sm focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500"
                    value={selectedLocalId}
                    onChange={(e) => setSelectedLocalId(e.target.value)}
                  >
                    {catalog?.models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.family} · {friendlyModelLabel(m.id)}
                      </option>
                    ))}
                  </select>
                </div>

                {selectedLocalModel && (
                  <div className="w-full bg-slate-50 rounded-2xl p-4 border border-slate-100">
                    <div className="flex text-left justify-between items-start mb-3">
                      <div>
                        <div className="text-sm font-bold text-slate-900 group flex items-center gap-2">
                          Capabilities & Specifications
                          <Info className="w-4 h-4 text-slate-400" />
                        </div>
                      </div>
                    </div>
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                      <div className="flex flex-col text-left">
                        <span className="text-slate-500 text-xs font-medium">Famiglia</span>
                        <span className="text-slate-900 font-semibold">{selectedLocalModel.family}</span>
                      </div>
                      <div className="flex flex-col text-left">
                        <span className="text-slate-500 text-xs font-medium">Backend Class</span>
                        <span className="text-slate-900 font-semibold">{selectedLocalModel.resolvedBackendClass || "n/a"}</span>
                      </div>
                      <div className="flex flex-col text-left">
                        <span className="text-slate-500 text-xs font-medium">Tokenizer</span>
                        <span className="text-slate-900 font-semibold">{selectedLocalModel.tokenizerPresent ? "Integrato" : "Assente"}</span>
                      </div>
                      <div className="flex flex-col text-left">
                        <span className="text-slate-500 text-xs font-medium">Resident KV</span>
                        <span className="text-slate-900 font-semibold">{selectedLocalModel.resolvedBackendCapabilities?.residentKv ? "Si" : "No"}</span>
                      </div>
                    </div>
                  </div>
                )}

                <button
                  onClick={() => handleLoadLocal(selectedLocalModel?.path ?? "")}
                  disabled={actionLoading || !selectedLocalModel}
                  className="flex items-center gap-2 bg-slate-900 text-white px-6 py-2.5 rounded-xl font-semibold hover:bg-slate-800 disabled:opacity-50 transition-colors"
                >
                  <Play className="w-4 h-4" />
                  Carica nel Runtime Locale
                </button>
              </div>
            )}

            {activeTab === "remote" && (
              <div className="space-y-6">
                 <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                    <div className="text-left w-full flex flex-col items-start gap-2">
                      <label className="block text-sm font-semibold text-slate-700">Provider Backend</label>
                      <select
                        className="w-full bg-slate-50 border border-slate-200 rounded-xl px-4 py-3 text-sm focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500"
                        value={selectedCloudProviderId}
                        onChange={(e) => {
                          setSelectedCloudProviderId(e.target.value);
                          const prod = catalog?.remoteProviders.find((p) => p.id === e.target.value);
                          if (prod && prod.models.length > 0) {
                            setSelectedCloudModelId(prod.models[0].id);
                          } else {
                            setSelectedCloudModelId("");
                          }
                        }}
                      >
                        {catalog?.remoteProviders.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.label} ({p.backendId})
                          </option>
                        ))}
                      </select>
                    </div>

                    <div className="text-left w-full flex flex-col items-start gap-2">
                       <label className="block text-sm font-semibold text-slate-700">Remote Model</label>
                       <select
                         className="w-full bg-slate-50 border border-slate-200 rounded-xl px-4 py-3 text-sm focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500"
                         value={selectedCloudModelId}
                         onChange={(e) => setSelectedCloudModelId(e.target.value)}
                         disabled={!selectedCloudProviderId}
                       >
                         {selectedCloudProvider?.models.map((m) => (
                           <option key={m.id} value={m.id}>
                             {m.label} ({m.id})
                           </option>
                         ))}
                       </select>
                    </div>
                 </div>

                 {selectedCloudModel && (
                   <div className="w-full text-left bg-slate-50 rounded-2xl p-4 border border-slate-100">
                     <div className="flex justify-between items-start mb-3">
                       <div>
                         <div className="text-sm font-bold flex items-center gap-2 text-slate-900">
                           Cloud Model Specifications
                           <Info className="w-4 h-4 text-slate-400" />
                         </div>
                       </div>
                     </div>
                     <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                       <div className="flex flex-col text-left">
                         <span className="text-slate-500 text-xs font-medium">Context Window</span>
                         <span className="text-slate-900 font-semibold">{selectedCloudModel.contextWindowTokens ? `${(selectedCloudModel.contextWindowTokens / 1000)}k` : "n/a"}</span>
                       </div>
                       <div className="flex flex-col text-left">
                         <span className="text-slate-500 text-xs font-medium">Max Output</span>
                         <span className="text-slate-900 font-semibold">{selectedCloudModel.maxOutputTokens ? `${(selectedCloudModel.maxOutputTokens / 1000)}k` : "n/a"}</span>
                       </div>
                       <div className="flex flex-col text-left">
                         <span className="text-slate-500 text-xs font-medium">Structured Output</span>
                         <span className="text-slate-900 font-semibold">{selectedCloudModel.supportsStructuredOutput ? "Supportato" : "Non supportato"}</span>
                       </div>
                       <div className="flex flex-col text-left">
                         <span className="text-slate-500 text-xs font-medium">Pricing</span>
                         <span className="text-slate-900 font-semibold">{selectedCloudModel.inputPriceUsdPerMtok ? `$${selectedCloudModel.inputPriceUsdPerMtok} / $${selectedCloudModel.outputPriceUsdPerMtok}` : "n/a"}</span>
                       </div>
                     </div>
                   </div>
                 )}

                 <button
                   onClick={handleLoadRemote}
                   disabled={actionLoading || !selectedCloudModel}
                   className="flex items-center gap-2 bg-indigo-600 text-white px-6 py-2.5 rounded-xl font-semibold hover:bg-indigo-700 disabled:opacity-50 transition-colors"
                 >
                   <Play className="w-4 h-4" />
                   Carica Target Remote
                 </button>
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
