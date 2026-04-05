import type { WorkspaceSnapshot } from "../../../lib/api";
import type { AgentSessionSummary } from "../../../store/sessions-store";
import { ArtifactsSection } from "./artifacts-section";
import { ContextSection } from "./context-section";
import { PermissionsSection } from "./permissions-section";
import { ReplayBranchSection } from "./replay-branch-section";
import { RuntimeSection } from "./runtime-section";

interface InspectDrawerProps {
  session: AgentSessionSummary;
  snapshot: WorkspaceSnapshot | null;
  compactionToast: string | null;
}

export function InspectDrawer({
  session,
  snapshot,
  compactionToast,
}: InspectDrawerProps) {
  return (
    <aside className="panel-surface flex h-full w-[410px] shrink-0 flex-col overflow-hidden">
      <div className="border-b border-slate-200 px-5 py-4">
        <div className="text-lg font-semibold tracking-tight text-slate-950">
          Inspect Session
        </div>
        <div className="mt-1 text-sm text-slate-500">
          Context first, then runtime, permissions and session artifacts.
        </div>
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto p-4">
        <ContextSection
          session={session}
          snapshot={snapshot}
          compactionToast={compactionToast}
        />
        <RuntimeSection session={session} snapshot={snapshot} />
        <PermissionsSection snapshot={snapshot} />
        <ArtifactsSection snapshot={snapshot} />
        <ReplayBranchSection snapshot={snapshot} />
      </div>
    </aside>
  );
}
