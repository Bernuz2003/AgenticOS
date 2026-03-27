import { Cpu, FileText } from "lucide-react";

import type { WorkspaceSnapshot } from "../../../lib/api";
import { friendlyRuntimeLabel } from "../../../lib/models/labels";
import { runtimeStateLabel, runtimeStateTone } from "../../../lib/utils/formatting";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import { formatValue } from "./format";

interface RuntimeCardProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  onOpenAudit: () => void;
}

export function RuntimeCard({ session, snapshot, onOpenAudit }: RuntimeCardProps) {
  const runtimeState = snapshot?.state ?? session.runtimeState ?? null;
  const runtimeLabel = friendlyRuntimeLabel(
    snapshot?.runtimeLabel ?? session.runtimeLabel ?? null,
    snapshot?.runtimeId ?? session.runtimeId ?? null,
  );
  const ownerId = snapshot?.ownerId ?? null;
  const toolCaller = snapshot?.toolCaller ?? null;
  const indexPos = snapshot?.indexPos ?? null;
  const priority = snapshot?.priority ?? null;
  const quotaTokens = snapshot?.quotaTokens ?? null;
  const quotaSyscalls = snapshot?.quotaSyscalls ?? null;
  const backendClass = snapshot?.backendClass ?? "unknown";
  const permissions = snapshot?.permissions ?? null;

  return (
    <>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-slate-900">
            Cognitive Telemetry
          </h2>
          <p className="mt-1 text-xs font-semibold uppercase tracking-wider text-slate-500">
            Real-time Analysis
          </p>
        </div>
        <button
          onClick={onOpenAudit}
          className="rounded-lg border border-slate-200 bg-white p-2 text-slate-600 shadow-sm transition-colors hover:border-indigo-200 hover:bg-indigo-50 hover:text-indigo-600"
          title="Open Technical Audit"
        >
          <FileText className="h-5 w-5" />
        </button>
      </div>

      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
              Runtime Control
            </div>
            <div className="mt-1 text-sm font-semibold text-slate-900">
              {formatValue(runtimeLabel)}
            </div>
          </div>
          <span
            className={`rounded-full border px-3 py-1 text-[11px] font-bold ${runtimeStateTone(
              runtimeState,
            )}`}
          >
            {runtimeStateLabel(runtimeState)}
          </span>
        </div>
        <div className="grid grid-cols-2 gap-3 text-xs">
          <div>
            <span className="mb-0.5 block text-slate-500">Priority</span>
            <span className="font-medium capitalize text-slate-900">
              {formatValue(priority)}
            </span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Owner</span>
            <span className="font-medium text-slate-900">{formatValue(ownerId)}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Token Cursor</span>
            <span className="font-medium text-slate-900">{formatValue(indexPos)}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Quota Tokens</span>
            <span className="font-medium text-slate-900">{formatValue(quotaTokens)}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Quota Syscalls</span>
            <span className="font-medium text-slate-900">{formatValue(quotaSyscalls)}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Backend</span>
            <span className="font-medium text-slate-900">{backendClass}</span>
          </div>
        </div>
      </section>

      <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
              Process Permissions
            </div>
            <div className="mt-1 text-sm font-semibold text-slate-900">
              {permissions?.trustScope ?? "unknown"}
            </div>
          </div>
          <span
            className={`rounded-full border px-3 py-1 text-[11px] font-bold ${
              permissions?.actionsAllowed
                ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                : "border-slate-200 bg-slate-50 text-slate-600"
            }`}
          >
            {permissions?.actionsAllowed ? "Actions enabled" : "Actions blocked"}
          </span>
        </div>
        <div className="grid grid-cols-1 gap-3 text-xs">
          <div>
            <span className="mb-0.5 block text-slate-500">Caller</span>
            <span className="font-medium text-slate-900">{formatValue(toolCaller)}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Path Scopes</span>
            <span className="font-medium text-slate-900">
              {permissions && permissions.pathScopes.length > 0
                ? permissions.pathScopes.join(", ")
                : "none"}
            </span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Allowed Tools</span>
            <span className="font-medium text-slate-900">
              {permissions && permissions.allowedTools.length > 0
                ? permissions.allowedTools.join(", ")
                : "none"}
            </span>
          </div>
        </div>
      </section>

      <div className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-3 flex items-center gap-2 text-sm font-bold text-slate-900">
          <Cpu className="h-4 w-4 text-indigo-500" />
          Runtime snapshot
        </div>
        <div className="grid grid-cols-2 gap-3 text-xs">
          <div>
            <span className="mb-0.5 block text-slate-500">Session PID</span>
            <span className="font-medium text-slate-900">{snapshot?.pid ?? session.pid}</span>
          </div>
          <div>
            <span className="mb-0.5 block text-slate-500">Active PID</span>
            <span className="font-medium text-slate-900">
              {snapshot?.activePid ?? session.activePid ?? "n/a"}
            </span>
          </div>
        </div>
      </div>
    </>
  );
}
