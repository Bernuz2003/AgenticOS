import { useEffect, useRef, useState } from "react";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import type { WorkspaceSnapshot } from "../../../lib/api";
import { ArtifactsCard } from "./artifacts-card";
import { AuditCard } from "./audit-card";
import { ContextCard } from "./context-card";
import { RuntimeCard } from "./runtime-card";

export function MindPanel({
  session,
  snapshot,
  loading,
  error,
  onOpenAudit,
}: {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  loading: boolean;
  error: string | null;
  onOpenAudit: () => void;
}) {
  const auditEvents = [...(snapshot?.auditEvents ?? [])].sort(
    (left, right) => right.recordedAtMs - left.recordedAtMs,
  );
  const [compactionToast, setCompactionToast] = useState<string | null>(null);
  const lastCompactionRef = useRef<string | null>(null);

  useEffect(() => {
    const currentReason = snapshot?.context?.lastCompactionReason ?? null;
    if (
      currentReason &&
      currentReason !== lastCompactionRef.current
    ) {
      setCompactionToast(currentReason);
      const timeout = window.setTimeout(() => setCompactionToast(null), 4000);
      lastCompactionRef.current = currentReason;
      return () => window.clearTimeout(timeout);
    }

    lastCompactionRef.current = currentReason;
    return undefined;
  }, [snapshot?.context?.lastCompactionReason]);

  return (
    <aside className="flex h-full min-h-0 w-full flex-shrink-0 flex-col gap-6 overflow-y-auto border-l border-slate-200 bg-slate-50 p-6 md:w-80 lg:w-96">
      <RuntimeCard session={session} snapshot={snapshot} onOpenAudit={onOpenAudit} />
      <ContextCard
        session={session}
        snapshot={snapshot}
        compactionToast={compactionToast}
      />
      <AuditCard auditEvents={auditEvents} onOpenAudit={onOpenAudit} />
      <ArtifactsCard snapshot={snapshot} />

      {loading && <div className="text-xs text-slate-400 text-center animate-pulse">Syncing telemetry data...</div>}
      {error && <div className="rounded-xl bg-rose-50 border border-rose-100 p-3 text-sm text-rose-600">{error}</div>}
    </aside>
  );
}
