import { ArrowRight, Plus, TimerReset, Trash2 } from "lucide-react";
import { Link } from "react-router-dom";

import type { AgentSessionSummary } from "../../store/sessions-store";
import { shortId } from "../../lib/utils/ids";

interface SessionsListProps {
  sessions: AgentSessionSummary[];
  deletingSessionId: string | null;
  onDelete: (sessionId: string) => void;
  onCreateSession: () => void;
}

export function SessionsList({
  sessions,
  deletingSessionId,
  onDelete,
  onCreateSession,
}: SessionsListProps) {
  if (sessions.length === 0) {
    return (
      <div className="rounded-3xl border-2 border-dashed border-slate-200 bg-slate-50/50 px-6 py-20 text-center">
        <p className="mb-4 font-medium text-slate-500">
          No sessions found in history database.
        </p>
        <button
          onClick={onCreateSession}
          className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-5 py-2.5 font-semibold text-slate-700 shadow-sm transition-colors hover:bg-slate-50 hover:text-indigo-600"
        >
          <Plus className="h-5 w-5" />
          Inizia la tua prima sessione
        </button>
      </div>
    );
  }

  return (
    <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
      {sessions.map((session) => (
        <div
          key={session.sessionId}
          className="group flex flex-col overflow-hidden rounded-[24px] border border-slate-200 bg-white shadow-sm transition-all hover:-translate-y-1 hover:shadow-md"
        >
          <div className="flex flex-1 flex-col p-6">
            <div className="mb-4 flex items-start justify-between gap-4">
              <div>
                <span className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
                  SESSION {shortId(session.sessionId.split("-")[1] || session.sessionId)}
                </span>
                <h2 className="mt-2 line-clamp-2 text-xl font-bold leading-tight text-slate-900">
                  {session.title}
                </h2>
              </div>
            </div>

            <div className="mb-6 flex-1 line-clamp-3 text-sm text-slate-600">
              {session.promptPreview}
              {session.runtimeState ? ` | state=${session.runtimeState}` : ""}
            </div>

            <div className="mb-6 grid grid-cols-2 gap-3">
              <div className="rounded-2xl border border-slate-100 bg-slate-50 p-3">
                <span className="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-slate-500">
                  Tokens
                </span>
                <span className="text-sm font-semibold text-slate-900">
                  {session.tokensLabel}
                </span>
              </div>
              <div className="rounded-2xl border border-slate-100 bg-slate-50 p-3">
                <span className="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-slate-500">
                  Uptime
                </span>
                <span className="flex items-center gap-1.5 text-sm font-semibold text-slate-900">
                  <TimerReset className="h-3.5 w-3.5 text-slate-400" />
                  {session.uptimeLabel}
                </span>
              </div>
            </div>

            <div className="flex items-center gap-2">
              <button
                onClick={() => onDelete(session.sessionId)}
                disabled={deletingSessionId === session.sessionId}
                className="rounded-xl p-3 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-600 disabled:opacity-50"
              >
                <Trash2 className="h-5 w-5" />
              </button>
              <Link
                to={`/workspace/${session.sessionId}`}
                className="flex flex-1 items-center justify-center gap-2 rounded-xl bg-indigo-50 px-4 py-3 font-semibold text-indigo-700 transition-colors hover:bg-indigo-100"
              >
                Resume
                <ArrowRight className="ml-1 h-4 w-4" />
              </Link>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
