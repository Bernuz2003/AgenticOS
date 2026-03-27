import { useSessionsStore } from "../../store/sessions-store";
import {
  Plus,
  X,
  LoaderCircle,
  Waypoints,
} from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { deleteSession, startSession } from "../../lib/api";
import { useState } from "react";
import { SessionsList } from "./list";

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
            onClick={() => setIsModalOpen(true)}
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
        onCreateSession={() => setIsModalOpen(true)}
      />

      {/* Modal Nuova Sessione */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 backdrop-blur-sm px-4">
          <div className="bg-white rounded-3xl shadow-xl w-full max-w-lg overflow-hidden animate-in zoom-in-95 duration-200">
             <div className="flex items-center justify-between px-6 py-4 border-b border-slate-100 bg-slate-50/50">
               <h3 className="text-lg font-bold text-slate-900">Inizia Nuova Chat</h3>
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
