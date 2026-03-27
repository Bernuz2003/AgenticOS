interface SettingsRuntimeSectionProps {
  selectedModelId: string;
  loadedModelId: string;
  loadedBackendClass: string | null;
}

export function SettingsRuntimeSection({
  selectedModelId,
  loadedModelId,
  loadedBackendClass,
}: SettingsRuntimeSectionProps) {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
        Runtime Defaults
      </div>
      <div className="mt-5 grid gap-4 md:grid-cols-3">
        <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
          <div className="text-xs text-slate-500">Selected</div>
          <div className="mt-1 text-sm font-semibold text-slate-900">
            {selectedModelId || "n/a"}
          </div>
        </div>
        <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
          <div className="text-xs text-slate-500">Loaded</div>
          <div className="mt-1 text-sm font-semibold text-slate-900">
            {loadedModelId || "n/a"}
          </div>
        </div>
        <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
          <div className="text-xs text-slate-500">Backend</div>
          <div className="mt-1 text-sm font-semibold text-slate-900">
            {loadedBackendClass || "n/a"}
          </div>
        </div>
      </div>
    </section>
  );
}
