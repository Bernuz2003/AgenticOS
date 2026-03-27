import type { ModelCatalogSnapshot } from "../../lib/api";
import { friendlyModelLabel } from "../../lib/models/labels";

interface ModelsListProps {
  catalog: ModelCatalogSnapshot | null;
  loading: boolean;
}

export function ModelsList({ catalog, loading }: ModelsListProps) {
  if (loading) {
    return (
      <div className="rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
        Loading model catalog...
      </div>
    );
  }

  if (!catalog) {
    return (
      <div className="rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
        No model catalog available.
      </div>
    );
  }

  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_360px]">
      <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Local Catalog
        </div>
        <div className="mt-5 grid gap-4 lg:grid-cols-2">
          {catalog.models.map((model) => (
            <article
              key={model.id}
              className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
            >
              <div className="text-sm font-semibold text-slate-900">
                {friendlyModelLabel(model.id)}
              </div>
              <div className="mt-1 text-xs text-slate-500">
                {model.family} · {model.resolvedBackendClass || "n/a"}
              </div>
              <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-slate-500">
                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                  tokenizer {model.tokenizerPresent ? "present" : "missing"}
                </span>
                <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1">
                  resident_kv {model.resolvedBackendCapabilities?.residentKv ? "yes" : "no"}
                </span>
              </div>
            </article>
          ))}
        </div>
      </section>

      <aside className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Remote Providers
        </div>
        <div className="mt-5 space-y-4">
          {catalog.remoteProviders.map((provider) => (
            <div
              key={provider.id}
              className="rounded-2xl border border-slate-200 bg-slate-50 p-4"
            >
              <div className="text-sm font-semibold text-slate-900">
                {provider.label}
              </div>
              <div className="mt-1 text-xs text-slate-500">
                backend {provider.backendId}
              </div>
              <div className="mt-3 text-xs text-slate-600">
                {provider.models.length} models exposed
              </div>
            </div>
          ))}
        </div>
      </aside>
    </div>
  );
}
