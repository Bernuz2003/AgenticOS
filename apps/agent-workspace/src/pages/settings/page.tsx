import { useSessionsStore } from "../../store/sessions-store";
import { SettingsGeneralSection } from "./sections/general";
import { SettingsRuntimeSection } from "./sections/runtime";

export function SettingsPage() {
  const connected = useSessionsStore((state) => state.connected);
  const error = useSessionsStore((state) => state.error);
  const selectedModelId = useSessionsStore((state) => state.selectedModelId);
  const loadedModelId = useSessionsStore((state) => state.loadedModelId);
  const loadedBackendClass = useSessionsStore((state) => state.loadedBackendClass);

  return (
    <div className="mx-auto max-w-7xl space-y-8">
      <header className="rounded-3xl border border-slate-200 bg-white px-8 py-7 shadow-sm">
        <div className="text-xs font-bold uppercase tracking-[0.25em] text-slate-400">
          Settings
        </div>
        <h1 className="mt-2 text-3xl font-bold tracking-tight text-slate-900">
          Workspace configuration
        </h1>
      </header>

      <div className="grid gap-6">
        <SettingsGeneralSection connected={connected} error={error} />
        <SettingsRuntimeSection
          selectedModelId={selectedModelId}
          loadedModelId={loadedModelId}
          loadedBackendClass={loadedBackendClass}
        />
      </div>
    </div>
  );
}
