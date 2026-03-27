import { useEffect, useState } from "react";

import { listModels, type ModelCatalogSnapshot } from "../../lib/api";
import { useModelsStore } from "../../store/models-store";
import { ModelsList } from "./list";
import { ModelsRuntimeStatus } from "./runtime-status";

export function ModelsPage() {
  const {
    selectedModelId,
    loadedModelId,
    loadedTargetKind,
    loadedBackendId,
    loadedBackendClass,
  } = useModelsStore();
  const [catalog, setCatalog] = useState<ModelCatalogSnapshot | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      setLoading(true);
      try {
        const snapshot = await listModels();
        if (!cancelled) {
          setCatalog(snapshot);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="rounded-3xl border border-slate-200 bg-white px-8 py-7 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.25em] text-slate-400">
          Models
        </div>
        <h1 className="mt-2 text-3xl font-bold tracking-tight text-slate-900">
          Catalog and runtime targets
        </h1>
      </header>

      <ModelsRuntimeStatus
        selectedModelId={selectedModelId || null}
        loadedModelId={loadedModelId || null}
        loadedTargetKind={loadedTargetKind}
        loadedBackendId={loadedBackendId}
        loadedBackendClass={loadedBackendClass}
      />

      <ModelsList catalog={catalog} loading={loading} />
    </div>
  );
}
