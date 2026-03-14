import { useSessionsStore } from "../store/sessions-store";
import { TimerReset, Trash2, ArrowRight, Plus, X, LoaderCircle } from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { statusTone } from "../lib/format";
import { deleteSession, startSession } from "../lib/api";
import { useState } from "react";

export function SessionsPage() {
  const sessions = useSessionsStore((state) => state.sessions);
  const refreshLobby = useSessionsStore((state) => state.refresh);
  const [isDeleting, setIsDeleting] = useState<string | null>(null);
  
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [newPrompt, setNewPrompt] = useState("");
  const [isStarting, setIsStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const navigate = useNavigate();

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
    if (!newPrompt.trim()) return;

    try {
      setIsStarting(true);
      setStartError(null);
      const res = await startSession(newPrompt, "sliding_window");
      await refreshLobby();
      setIsModalOpen(false);
      setNewPrompt("");
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
          <h1 className="text-3xl font-bold tracking-tight text-slate-900">Session Hub</h1>
          <p className="text-slate-500 mt-2">Manage your active and past interactions with AgenticOS.</p>
        </div>
        <button
          onClick={() => setIsModalOpen(true)}
          className="flex items-center gap-2 bg-indigo-600 text-white px-5 py-2.5 rounded-xl font-semibold hover:bg-indigo-700 transition-colors shadow-sm"
        >
          <Plus className="w-5 h-5" />
          Nuova Sessione
        </button>
      </div>

      {sessions.length === 0 ? (
        <div className="text-center py-20 px-6 border-2 border-dashed border-slate-200 rounded-3xl bg-slate-50/50">
          <p className="text-slate-500 font-medium mb-4">No sessions found in history database.</p>
          <button
            onClick={() => setIsModalOpen(true)}
            className="inline-flex items-center gap-2 bg-white border border-slate-200 text-slate-700 px-5 py-2.5 rounded-xl font-semibold hover:bg-slate-50 hover:text-indigo-600 transition-colors shadow-sm"
          >
            <Plus className="w-5 h-5" />
            Inizia la tua prima sessione
          </button>
        </div>
      ) : (
        <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
          {sessions.map((session) => (
            <div
              key={session.sessionId}
              className="bg-white rounded-[24px] border border-slate-200 shadow-sm overflow-hidden flex flex-col group transition-all hover:-translate-y-1 hover:shadow-md"
            >
              <div className="p-6 flex-1 flex flex-col">
                <div className="flex items-start justify-between gap-4 mb-4">
                  <div>
                    <span className="text-xs uppercase tracking-[0.2em] font-bold text-slate-400">
                      SESSION {session.sessionId.split('-')[1] || session.sessionId.substring(0, 8)}
                    </span>
                    <h2 className="text-xl font-bold text-slate-900 mt-2 leading-tight line-clamp-2">
                      {session.title}
                    </h2>
                  </div>
                  <span className={`status-pill ${statusTone(session.status)}`}>
                    {session.status}
                  </span>
                </div>

                <div className="text-sm text-slate-600 line-clamp-3 mb-6 flex-1">
                  {session.promptPreview}
                </div>

                <div className="grid grid-cols-2 gap-3 mb-6">
                  <div className="bg-slate-50 rounded-2xl p-3 border border-slate-100">
                    <span className="text-[10px] uppercase tracking-wider font-semibold text-slate-500 block mb-1">Tokens</span>
                    <span className="font-semibold text-slate-900 text-sm">{session.tokensLabel}</span>
                  </div>
                  <div className="bg-slate-50 rounded-2xl p-3 border border-slate-100">
                    <span className="text-[10px] uppercase tracking-wider font-semibold text-slate-500 block mb-1">Uptime</span>
                    <span className="font-semibold flex items-center gap-1.5 text-slate-900 text-sm">
                      <TimerReset className="w-3.5 h-3.5 text-slate-400" />
                      {session.uptimeLabel}
                    </span>
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    onClick={() => handleDelete(session.sessionId)}
                    disabled={isDeleting === session.sessionId}
                    className="p-3 text-slate-400 hover:text-red-600 hover:bg-red-50 rounded-xl transition-colors disabled:opacity-50"
                  >
                    <Trash2 className="w-5 h-5" />
                  </button>
                  <Link
                    to={`/workspace/${session.sessionId}`}
                    className="flex-1 flex items-center justify-center gap-2 bg-indigo-50 text-indigo-700 font-semibold py-3 px-4 rounded-xl hover:bg-indigo-100 transition-colors"
                  >
                    Resume
                    <ArrowRight className="w-4 h-4 ml-1" />
                  </Link>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Modal Nuova Sessione */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 backdrop-blur-sm px-4">
          <div className="bg-white rounded-3xl shadow-xl w-full max-w-lg overflow-hidden animate-in zoom-in-95 duration-200">
             <div className="flex items-center justify-between px-6 py-4 border-b border-slate-100 bg-slate-50/50">
               <h3 className="text-lg font-bold text-slate-900">Inizia Nuova Sessione</h3>
               <button 
                 onClick={() => setIsModalOpen(false)}
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
               
               {startError && (
                 <div className="mb-4 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
                   {startError}
                 </div>
               )}

               <div className="flex justify-end gap-3 mt-4">
                 <button
                   type="button"
                   onClick={() => setIsModalOpen(false)}
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
