import { ArrowRight, TimerReset, Waypoints } from "lucide-react";
import { Link } from "react-router-dom";
import { statusTone, strategyLabel } from "../../lib/format";
import type { AgentSessionSummary } from "../../store/sessions-store";

export function SessionCard({ session }: { session: AgentSessionSummary }) {
  return (
    <Link
      to={`/workspace/${session.sessionId}`}
      className="panel-surface group flex min-h-[270px] flex-col justify-between overflow-hidden p-6 transition duration-200 hover:-translate-y-1 hover:border-slate-900/10 hover:bg-white/80"
    >
      <div className="space-y-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
              PID {session.pid}
            </p>
            <h2 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
              {session.title}
            </h2>
            {session.orchestrationId ? (
              <p className="mt-2 text-xs font-semibold uppercase tracking-[0.22em] text-cyan-700">
                orch {session.orchestrationId} · task {session.orchestrationTaskId || "n/a"}
              </p>
            ) : null}
          </div>
          <span className={`status-pill ${statusTone(session.status)}`}>
            {session.status}
          </span>
        </div>

        <p className="max-w-sm text-sm leading-6 text-slate-600">
          {session.promptPreview}
        </p>
      </div>

      <div className="space-y-5">
        <div className="grid grid-cols-3 gap-3 text-sm text-slate-600">
          <div className="rounded-2xl bg-slate-950/[0.04] p-3">
            <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">Uptime</div>
            <div className="mt-2 flex items-center gap-2 font-semibold text-slate-900">
              <TimerReset className="h-4 w-4" />
              {session.uptimeLabel}
            </div>
          </div>
          <div className="rounded-2xl bg-slate-950/[0.04] p-3">
            <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">Tokens</div>
            <div className="mt-2 font-semibold text-slate-900">{session.tokensLabel}</div>
          </div>
          <div className="rounded-2xl bg-slate-950/[0.04] p-3">
            <div className="text-[11px] uppercase tracking-[0.22em] text-slate-500">Strategy</div>
            <div className="mt-2 flex items-center gap-2 font-semibold text-slate-900">
              <Waypoints className="h-4 w-4" />
              {strategyLabel(session.contextStrategy)}
            </div>
          </div>
        </div>

        <div className="flex items-center justify-between text-sm font-semibold text-slate-950">
          <span>Apri workspace</span>
          <ArrowRight className="h-4 w-4 transition group-hover:translate-x-1" />
        </div>
      </div>
    </Link>
  );
}