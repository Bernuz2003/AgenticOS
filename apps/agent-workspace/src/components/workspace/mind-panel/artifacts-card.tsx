import { BarChart3 } from "lucide-react";

import type { WorkspaceSnapshot } from "../../../lib/api";

interface ArtifactsCardProps {
  snapshot: WorkspaceSnapshot | null;
}

export function ArtifactsCard({ snapshot }: ArtifactsCardProps) {
  const accounting = snapshot?.accounting ?? null;

  return (
    <>
      <div className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-3 flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-500">
          <BarChart3 className="h-4 w-4 text-indigo-400" />
          Session Accounting
        </div>
        {accounting ? (
          <div className="grid grid-cols-2 gap-3 text-sm">
            <div>
              <span className="block text-xs text-slate-500">Costs</span>
              <span className="font-bold text-emerald-600">
                ${accounting.estimatedCostUsd.toFixed(6)}
              </span>
            </div>
            <div>
              <span className="block text-xs text-slate-500">Requests</span>
              <span className="font-bold text-slate-900">{accounting.requestsTotal}</span>
            </div>
            <div>
              <span className="block text-xs text-slate-500">Tokens IN/OUT</span>
              <span className="font-semibold text-slate-700">
                {accounting.inputTokensTotal} / {accounting.outputTokensTotal}
              </span>
            </div>
            <div>
              <span className="block text-xs text-slate-500">Errors</span>
              <span className="font-semibold text-rose-600">
                {accounting.rateLimitErrors}/{accounting.authErrors}/
                {accounting.transportErrors}
              </span>
            </div>
          </div>
        ) : (
          <div className="py-2 text-center text-sm italic text-slate-500">
            No recorded accounting data
          </div>
        )}
      </div>

      {snapshot?.orchestration && (
        <section className="rounded-2xl border border-indigo-200 bg-indigo-50 p-4">
          <div className="mb-2 flex items-center justify-between text-sm font-bold text-indigo-900">
            <span>Orchestration {snapshot.orchestration.orchestrationId}</span>
            <span className="status-pill bg-indigo-100 text-xs text-indigo-700">
              {snapshot.orchestration.policy}
            </span>
          </div>
          <div className="mb-3 truncate text-sm text-indigo-800">
            Task: <span className="font-semibold">{snapshot.orchestration.taskId}</span>
          </div>
          <div className="grid grid-cols-4 gap-2 text-center text-xs">
            <div className="flex flex-col rounded-lg bg-white py-2 font-semibold text-indigo-900 shadow-sm">
              <span className="text-[10px] uppercase text-indigo-400">Run</span>
              {snapshot.orchestration.running}
            </div>
            <div className="flex flex-col rounded-lg bg-white py-2 font-semibold text-indigo-900 shadow-sm">
              <span className="text-[10px] uppercase text-indigo-400">Wait</span>
              {snapshot.orchestration.pending}
            </div>
            <div className="flex flex-col rounded-lg bg-white py-2 font-semibold text-indigo-900 shadow-sm">
              <span className="text-[10px] uppercase text-indigo-400">Done</span>
              {snapshot.orchestration.completed}
            </div>
            <div className="flex flex-col rounded-lg bg-white py-2 font-semibold text-rose-600 shadow-sm">
              <span className="text-[10px] uppercase text-rose-300">Fail</span>
              {snapshot.orchestration.failed}
            </div>
          </div>
        </section>
      )}
    </>
  );
}
