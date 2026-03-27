import { formatBackendReference, formatModelReference } from "../../lib/models/formatting";

interface ModelsRuntimeStatusProps {
  selectedModelId: string | null;
  loadedModelId: string | null;
  loadedTargetKind: string | null;
  loadedBackendId: string | null;
  loadedBackendClass: string | null;
}

export function ModelsRuntimeStatus({
  selectedModelId,
  loadedModelId,
  loadedTargetKind,
  loadedBackendId,
  loadedBackendClass,
}: ModelsRuntimeStatusProps) {
  return (
    <section className="grid gap-4 md:grid-cols-3">
      <div className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Selected Model
        </div>
        <div className="mt-3 text-lg font-bold text-slate-900">
          {formatModelReference(selectedModelId)}
        </div>
      </div>
      <div className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Loaded Target
        </div>
        <div className="mt-3 text-lg font-bold text-slate-900">
          {formatModelReference(loadedModelId)}
        </div>
        <div className="mt-1 text-sm text-slate-500">{loadedTargetKind || "n/a"}</div>
      </div>
      <div className="rounded-3xl border border-slate-200 bg-white p-5 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
          Backend
        </div>
        <div className="mt-3 text-lg font-bold text-slate-900">
          {formatBackendReference(loadedBackendClass, loadedBackendId)}
        </div>
      </div>
    </section>
  );
}
