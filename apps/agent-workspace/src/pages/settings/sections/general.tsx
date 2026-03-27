interface SettingsGeneralSectionProps {
  connected: boolean;
  error: string | null;
}

export function SettingsGeneralSection({
  connected,
  error,
}: SettingsGeneralSectionProps) {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
        Bridge
      </div>
      <div className="mt-3 text-lg font-bold text-slate-900">
        {connected ? "Connected" : "Disconnected"}
      </div>
      <div className="mt-2 text-sm text-slate-500">{error ?? "Realtime bridge healthy."}</div>
    </section>
  );
}
