import { LoaderCircle, Plus } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { startSession } from "../../lib/api";
import { useSessionsStore } from "../../store/sessions-store";

export function NewAgentCard() {
  const navigate = useNavigate();
  const refreshSessions = useSessionsStore((state) => state.refresh);
  const [prompt, setPrompt] = useState("");
  const [workload, setWorkload] = useState("auto");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleStart() {
    const trimmedPrompt = prompt.trim();
    if (!trimmedPrompt) {
      setError("Inserisci un prompt per avviare una nuova sessione.");
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const session = await startSession(trimmedPrompt, workload);
      await refreshSessions();
      navigate(`/workspace/${session.sessionId}`);
    } catch (startError) {
      setError(
        startError instanceof Error ? startError.message : "Failed to start session",
      );
    } finally {
      setLoading(false);
    }
  }

  return (
    <div
      id="new-agent-card"
      className="panel-surface flex min-h-[270px] flex-col justify-between border-dashed p-6 text-left transition duration-200 hover:-translate-y-1 hover:border-slate-900/20 hover:bg-white/85"
    >
      <div className="space-y-3">
        <div className="inline-flex h-12 w-12 items-center justify-center rounded-2xl bg-slate-950 text-white">
          <Plus className="h-6 w-6" />
        </div>
        <div>
          <h2 className="text-2xl font-bold tracking-tight text-slate-950">Nuova sessione</h2>
          <p className="mt-2 max-w-sm text-sm leading-6 text-slate-600">
            Avvia una nuova sessione `EXEC` dal bridge Tauri e apri subito la Workspace con Timeline live per il PID appena creato.
          </p>
        </div>
      </div>

      <div className="space-y-3 rounded-2xl bg-slate-950/[0.04] p-4 text-sm text-slate-700">
        <textarea
          id="new-agent-prompt"
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          placeholder="Scrivi il prompt iniziale della sessione"
          className="min-h-28 w-full rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-900 outline-none transition focus:border-slate-400"
        />
        <div className="flex items-center gap-3">
          <select
            value={workload}
            onChange={(event) => setWorkload(event.target.value)}
            className="rounded-full border border-slate-200 bg-white px-4 py-2 text-sm text-slate-900 outline-none"
          >
            <option value="auto">auto</option>
            <option value="fast">fast</option>
            <option value="code">code</option>
            <option value="reasoning">reasoning</option>
            <option value="general">general</option>
          </select>
          <button
            onClick={() => void handleStart()}
            disabled={loading}
            className="inline-flex items-center gap-2 rounded-full bg-slate-950 px-4 py-2 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {loading ? <LoaderCircle className="h-4 w-4 animate-spin" /> : null}
            {loading ? "Avvio..." : "Nuova sessione"}
          </button>
        </div>
        {error ? <p className="text-xs text-rose-700">{error}</p> : null}
      </div>
    </div>
  );
}
