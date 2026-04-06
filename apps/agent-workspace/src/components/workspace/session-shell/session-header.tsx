import { Bug, FileText, ArrowLeft } from "lucide-react";
import { Link } from "react-router-dom";

import type { WorkspaceSnapshot } from "../../../lib/api";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import type { WorkspaceMode } from "../../../lib/workspace/view-state";

interface SessionHeaderProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  mode: WorkspaceMode;
  onSelectBranch: (sessionId: string) => void;
  onOpenAudit: () => void;
  onToggleWorkspaceMode: () => void;
}

export function SessionHeader({
  session,
  snapshot,
  mode,
  onSelectBranch,
  onOpenAudit,
  onToggleWorkspaceMode,
}: SessionHeaderProps) {
  const title = snapshot?.title ?? session.title;
  const workspaceLabel = mode === "debug" ? "Core dump workspace" : "Conversation workspace";
  const lineage = snapshot?.lineage ?? null;
  const selectedBranch = lineage?.branches.find((branch) => branch.selected) ?? null;
  const showBranchSwitcher = (lineage?.branches.length ?? 0) > 1;
  const isReplayBranch = lineage?.selectedKind === "replay" && snapshot?.replay;

  return (
    <header className="panel-surface flex flex-col gap-4 px-5 py-4 md:px-6 md:py-5">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-3">
            <Link
              to="/sessions"
              className="inline-flex h-10 w-10 items-center justify-center rounded-2xl border border-slate-200 bg-white text-slate-500 transition hover:border-slate-300 hover:text-slate-900"
              title="Return to Sessions"
            >
              <ArrowLeft className="h-4 w-4" />
            </Link>
            <div className="min-w-0">
              <h1 className="truncate text-2xl font-semibold tracking-tight text-slate-950">
                {title}
              </h1>
              <p className="mt-1 text-sm text-slate-500">{workspaceLabel}</p>
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {showBranchSwitcher ? (
            <label className="flex min-w-[240px] flex-col gap-1">
              <span className="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-400">
                Branch
              </span>
              <select
                value={lineage?.selectedSessionId ?? session.sessionId}
                onChange={(event) => onSelectBranch(event.target.value)}
                className="rounded-2xl border border-slate-200 bg-white px-4 py-2 text-sm font-medium text-slate-800 outline-none transition focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10"
              >
                {lineage?.branches.map((branch) => (
                  <option key={branch.sessionId} value={branch.sessionId}>
                    {formatBranchKind(branch.kind)} · {branch.title}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
          <HeaderAction
            active={mode === "debug"}
            label={mode === "debug" ? "Conversation" : "Core Dumps"}
            icon={Bug}
            onClick={onToggleWorkspaceMode}
          />
          <HeaderAction
            active={false}
            label="Audit"
            icon={FileText}
            onClick={onOpenAudit}
          />
        </div>
      </div>

      {isReplayBranch ? (
        <div className="rounded-[24px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-950">
          <div className="font-semibold">
            Replay branch active{selectedBranch ? ` · ${selectedBranch.title}` : ""}
          </div>
          <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-amber-900/80">
            <span>Source dump: {snapshot.replay?.sourceDumpId ?? "n/a"}</span>
            <span>Mode: {snapshot.replay?.replayMode ?? "n/a"}</span>
            <span>Tool mode: {snapshot.replay?.toolMode ?? "n/a"}</span>
          </div>
        </div>
      ) : null}
    </header>
  );
}

function formatBranchKind(kind: NonNullable<WorkspaceSnapshot["lineage"]>["selectedKind"]): string {
  switch (kind) {
    case "replay":
      return "Replay";
    case "fork":
      return "Fork";
    default:
      return "Base";
  }
}

function HeaderAction({
  active,
  label,
  icon: Icon,
  onClick,
}: {
  active: boolean;
  label: string;
  icon: typeof Bug;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`inline-flex items-center gap-2 rounded-2xl border px-4 py-2 text-sm font-semibold transition ${
        active
          ? "border-slate-900 bg-slate-950 text-white"
          : "border-slate-200 bg-white text-slate-700 hover:border-slate-300 hover:text-slate-950"
      }`}
    >
      <Icon className="h-4 w-4" />
      {label}
    </button>
  );
}
