import { useSessionsStore } from "../../store/sessions-store";
import {
  Plus,
  X,
  LoaderCircle,
  Waypoints,
} from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import {
  deleteSession,
  startSession,
  type PathGrantAccessMode,
  type PathGrantInput,
} from "../../lib/api";
import { useState } from "react";
import { SessionsList } from "./list";

type QuotaMode = "unlimited" | "limit";
type GrantCapsule = "repo" | "docs" | "downloads" | "watch_folder" | "general";

interface SessionGrantDraft {
  id: string;
  root: string;
  accessMode: PathGrantAccessMode;
  capsule: GrantCapsule;
  label: string;
}

const GRANT_CAPSULE_OPTIONS: Array<{ value: GrantCapsule; label: string }> = [
  { value: "repo", label: "Repo" },
  { value: "docs", label: "Docs" },
  { value: "downloads", label: "Downloads" },
  { value: "watch_folder", label: "Watch Folder" },
  { value: "general", label: "General" },
];

function createEmptyGrantDraft(index: number): SessionGrantDraft {
  return {
    id: `grant-${index}`,
    root: "",
    accessMode: "read_only",
    capsule: "general",
    label: "",
  };
}

interface SessionQuotaFieldProps {
  label: string;
  description: string;
  mode: QuotaMode;
  value: string;
  disabled: boolean;
  onModeChange: (mode: QuotaMode) => void;
  onValueChange: (value: string) => void;
}

function SessionQuotaField({
  label,
  description,
  mode,
  value,
  disabled,
  onModeChange,
  onValueChange,
}: SessionQuotaFieldProps) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-slate-50/70 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-900">{label}</div>
          <div className="mt-1 text-xs leading-relaxed text-slate-500">{description}</div>
        </div>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-2">
        <button
          type="button"
          disabled={disabled}
          onClick={() => onModeChange("unlimited")}
          className={`rounded-xl border px-4 py-2.5 text-sm font-semibold transition ${
            mode === "unlimited"
              ? "border-emerald-300 bg-emerald-50 text-emerald-800"
              : "border-slate-200 bg-white text-slate-600 hover:border-slate-300"
          } disabled:opacity-50`}
        >
          No Limit
        </button>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onModeChange("limit")}
          className={`rounded-xl border px-4 py-2.5 text-sm font-semibold transition ${
            mode === "limit"
              ? "border-indigo-300 bg-indigo-50 text-indigo-800"
              : "border-slate-200 bg-white text-slate-600 hover:border-slate-300"
          } disabled:opacity-50`}
        >
          Limit
        </button>
      </div>

      {mode === "limit" && (
        <div className="mt-3">
          <input
            type="number"
            min={1}
            step={1}
            inputMode="numeric"
            value={value}
            disabled={disabled}
            onChange={(event) => onValueChange(event.target.value)}
            placeholder="Inserisci un limite positivo"
            className="w-full rounded-xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-100"
          />
        </div>
      )}
    </div>
  );
}

function parseQuotaInput(
  label: string,
  mode: QuotaMode,
  value: string,
): { value: number | null; error: string | null } {
  if (mode === "unlimited") {
    return { value: null, error: null };
  }

  const normalized = value.trim();
  if (!normalized) {
    return {
      value: null,
      error: `${label}: inserisci un numero intero positivo oppure seleziona No Limit.`,
    };
  }

  const parsed = Number(normalized);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    return {
      value: null,
      error: `${label}: il limite deve essere un intero positivo.`,
    };
  }

  return { value: parsed, error: null };
}

function normalizeGrantInput(
  grants: SessionGrantDraft[],
): { value: PathGrantInput[]; error: string | null } {
  const value: PathGrantInput[] = [];

  for (const [index, grant] of grants.entries()) {
    const root = grant.root.trim();
    if (!root) {
      return {
        value: [],
        error: `Grant ${index + 1}: inserisci un path assoluto o relativo al workspace.`,
      };
    }

    value.push({
      root,
      accessMode: grant.accessMode,
      capsule: grant.capsule,
      label: grant.label.trim() || null,
    });
  }

  return { value, error: null };
}

function SessionGrantEditor({
  grants,
  disabled,
  onAdd,
  onRemove,
  onChange,
}: {
  grants: SessionGrantDraft[];
  disabled: boolean;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onChange: (id: string, patch: Partial<SessionGrantDraft>) => void;
}) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-slate-50/70 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-900">Local Filesystem Grants</div>
          <div className="mt-1 text-xs leading-relaxed text-slate-500">
            Aggiungi root locali esplicite fuori da `workspace/`. I tool confinati rispettano questi
            grants; gli executor host non confinabili vengono esclusi automaticamente.
          </div>
        </div>
        <button
          type="button"
          disabled={disabled}
          onClick={onAdd}
          className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs font-semibold text-slate-700 transition hover:border-slate-300 hover:text-slate-950 disabled:opacity-50"
        >
          Add Grant
        </button>
      </div>

      {grants.length === 0 ? (
        <div className="mt-4 rounded-2xl border border-dashed border-slate-200 bg-white/70 px-4 py-4 text-sm text-slate-500">
          Nessun grant esterno configurato. La sessione resta confinata al workspace implicito.
        </div>
      ) : (
        <div className="mt-4 space-y-3">
          {grants.map((grant, index) => (
            <div
              key={grant.id}
              className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm"
            >
              <div className="flex items-center justify-between gap-3">
                <div className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                  Grant {index + 1}
                </div>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => onRemove(grant.id)}
                  className="rounded-full p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700 disabled:opacity-50"
                  aria-label={`Remove grant ${index + 1}`}
                >
                  <X className="h-4 w-4" />
                </button>
              </div>

              <div className="mt-3 grid gap-3 md:grid-cols-2">
                <label className="block">
                  <div className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                    Root Path
                  </div>
                  <input
                    type="text"
                    value={grant.root}
                    disabled={disabled}
                    onChange={(event) => onChange(grant.id, { root: event.target.value })}
                    placeholder="/path/to/repo oppure docs/reference"
                    className="w-full rounded-xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-100"
                  />
                </label>

                <label className="block">
                  <div className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                    Access Mode
                  </div>
                  <select
                    value={grant.accessMode}
                    disabled={disabled}
                    onChange={(event) =>
                      onChange(grant.id, {
                        accessMode: event.target.value as PathGrantAccessMode,
                      })
                    }
                    className="w-full rounded-xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-100"
                  >
                    <option value="read_only">read_only</option>
                    <option value="write_approved">write_approved</option>
                    <option value="autonomous_write">autonomous_write</option>
                  </select>
                </label>

                <label className="block">
                  <div className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                    Capsule
                  </div>
                  <select
                    value={grant.capsule}
                    disabled={disabled}
                    onChange={(event) =>
                      onChange(grant.id, {
                        capsule: event.target.value as GrantCapsule,
                      })
                    }
                    className="w-full rounded-xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-100"
                  >
                    {GRANT_CAPSULE_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="block">
                  <div className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                    Label
                  </div>
                  <input
                    type="text"
                    value={grant.label}
                    disabled={disabled}
                    onChange={(event) => onChange(grant.id, { label: event.target.value })}
                    placeholder="Optional human label"
                    className="w-full rounded-xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-100"
                  />
                </label>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function SessionsPage() {
  const sessions = useSessionsStore((state) => state.sessions);
  const refreshLobby = useSessionsStore((state) => state.refresh);
  const [isDeleting, setIsDeleting] = useState<string | null>(null);

  const [isModalOpen, setIsModalOpen] = useState(false);
  const [newPrompt, setNewPrompt] = useState("");
  const [tokenQuotaMode, setTokenQuotaMode] = useState<QuotaMode>("unlimited");
  const [tokenQuotaValue, setTokenQuotaValue] = useState("");
  const [syscallQuotaMode, setSyscallQuotaMode] = useState<QuotaMode>("unlimited");
  const [syscallQuotaValue, setSyscallQuotaValue] = useState("");
  const [pathGrants, setPathGrants] = useState<SessionGrantDraft[]>([]);
  const [isStarting, setIsStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const navigate = useNavigate();

  const closeStartModal = () => {
    setIsModalOpen(false);
    setNewPrompt("");
    setTokenQuotaMode("unlimited");
    setTokenQuotaValue("");
    setSyscallQuotaMode("unlimited");
    setSyscallQuotaValue("");
    setPathGrants([]);
    setStartError(null);
  };

  const addPathGrant = () => {
    setPathGrants((current) => [...current, createEmptyGrantDraft(current.length + 1)]);
  };

  const updatePathGrant = (id: string, patch: Partial<SessionGrantDraft>) => {
    setPathGrants((current) =>
      current.map((grant) => (grant.id === id ? { ...grant, ...patch } : grant)),
    );
  };

  const removePathGrant = (id: string) => {
    setPathGrants((current) => current.filter((grant) => grant.id !== id));
  };

  const handleDelete = async (sessionId: string) => {
    if (!confirm("Are you sure you want to permanently delete this session?")) return;

    try {
      setIsDeleting(sessionId);
      await deleteSession(sessionId);
      await refreshLobby();
    } catch (e) {
      console.error("Failed to delete session", e);
      alert(e);
    } finally {
      setIsDeleting(null);
    }
  };

  const handleStartSession = async (e: React.FormEvent) => {
    e.preventDefault();
    const prompt = newPrompt.trim();
    if (!prompt) return;

    const tokenQuota = parseQuotaInput("Quota token", tokenQuotaMode, tokenQuotaValue);
    if (tokenQuota.error) {
      setStartError(tokenQuota.error);
      return;
    }

    const syscallQuota = parseQuotaInput(
      "Quota syscall",
      syscallQuotaMode,
      syscallQuotaValue,
    );
    if (syscallQuota.error) {
      setStartError(syscallQuota.error);
      return;
    }

    const grants = normalizeGrantInput(pathGrants);
    if (grants.error) {
      setStartError(grants.error);
      return;
    }

    try {
      setIsStarting(true);
      setStartError(null);
      const res = await startSession({
        prompt,
        quotaTokens: tokenQuota.value,
        quotaSyscalls: syscallQuota.value,
        pathGrants: grants.value,
      });
      await refreshLobby();
      closeStartModal();
      navigate(`/workspace/${res.sessionId}`);
    } catch (error) {
      console.error("Failed to start session:", error);
      setStartError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsStarting(false);
    }
  };

  return (
    <div className="max-w-6xl mx-auto space-y-8 relative">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight text-slate-900">Chat Sessions</h1>
          <p className="text-slate-500 mt-2">
            Interactive chats stay here. Workflow orchestration now lives in a
            dedicated control-plane view.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Link
            to="/workflows"
            className="flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-5 py-2.5 font-semibold text-slate-700 hover:bg-slate-50 transition-colors shadow-sm"
          >
           <Waypoints className="w-5 h-5" />
            New Workflow
          </Link>
          <button
            onClick={() => {
              setStartError(null);
              setIsModalOpen(true);
            }}
            className="flex items-center gap-2 bg-indigo-600 text-white px-5 py-2.5 rounded-xl font-semibold hover:bg-indigo-700 transition-colors shadow-sm"
          >
            <Plus className="w-5 h-5" />
            Nuova Chat
          </button>
        </div>
      </div>

      <SessionsList
        sessions={sessions}
        deletingSessionId={isDeleting}
        onDelete={handleDelete}
        onCreateSession={() => {
          setStartError(null);
          setIsModalOpen(true);
        }}
      />

      {/* Modal Nuova Sessione */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 backdrop-blur-sm px-4">
          <div className="bg-white rounded-3xl shadow-xl w-full max-w-lg overflow-hidden animate-in zoom-in-95 duration-200">
             <div className="flex items-center justify-between px-6 py-4 border-b border-slate-100 bg-slate-50/50">
               <h3 className="text-lg font-bold text-slate-900">Inizia Nuova Chat</h3>
               <button 
                 onClick={closeStartModal}
                 className="p-2 text-slate-400 hover:text-slate-600 hover:bg-slate-100 rounded-full transition-colors"
               >
                 <X className="w-5 h-5" />
               </button>
             </div>
             
             <form onSubmit={handleStartSession} className="p-6">
               <label className="block text-sm font-semibold text-slate-700 mb-2">Prompt Iniziale</label>
               <textarea
                 value={newPrompt}
                 onChange={(e) => setNewPrompt(e.target.value)}
                 disabled={isStarting}
                 placeholder="Di cosa vuoi parlare o quale task vuoi assegnare all'agente?"
                 className="w-full min-h-[120px] resize-y rounded-2xl border border-slate-300 bg-white px-4 py-3 text-sm leading-relaxed text-slate-800 outline-none focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-50 disabled:text-slate-500 mb-2"
                 autoFocus
               />

               <div className="mt-5 space-y-4">
                 <SessionQuotaField
                   label="Quota token"
                   description="Guardrail di sessione sui token generati. No Limit rimuove la quota della sessione, ma non alza automaticamente il cap tecnico di generation del singolo turno."
                   mode={tokenQuotaMode}
                   value={tokenQuotaValue}
                   disabled={isStarting}
                   onModeChange={setTokenQuotaMode}
                   onValueChange={setTokenQuotaValue}
                 />
                 <SessionQuotaField
                   label="Quota syscall"
                   description="Numero massimo di syscall o tool call consentite durante la sessione."
                   mode={syscallQuotaMode}
                   value={syscallQuotaValue}
                   disabled={isStarting}
                   onModeChange={setSyscallQuotaMode}
                   onValueChange={setSyscallQuotaValue}
                 />
                 <SessionGrantEditor
                   grants={pathGrants}
                   disabled={isStarting}
                   onAdd={addPathGrant}
                   onRemove={removePathGrant}
                   onChange={updatePathGrant}
                 />
               </div>

               {startError && (
                 <div className="mb-4 mt-4 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
                   {startError}
                 </div>
               )}

               <div className="flex justify-end gap-3 mt-4">
                 <button
                   type="button"
                   onClick={closeStartModal}
                   disabled={isStarting}
                   className="px-5 py-2.5 rounded-xl font-semibold text-slate-600 hover:bg-slate-100 transition-colors disabled:opacity-50"
                 >
                   Annulla
                 </button>
                 <button
                   type="submit"
                   disabled={isStarting || !newPrompt.trim()}
                   className="flex items-center gap-2 bg-indigo-600 text-white px-6 py-2.5 rounded-xl font-semibold hover:bg-indigo-700 disabled:opacity-50 transition-colors"
                 >
                   {isStarting ? (
                     <>
                       <LoaderCircle className="w-4 h-4 animate-spin" />
                       Avvio in corso...
                     </>
                   ) : (
                     "Avvia Sessione"
                   )}
                 </button>
               </div>
             </form>
          </div>
        </div>
      )}
    </div>
  );
}
