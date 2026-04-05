import { useEffect, useMemo, useState } from "react";

import type {
  CoreDumpInfo,
  CoreDumpSummary,
  WorkspaceReplayDebugSnapshot,
} from "../../../lib/api";
import { parseCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import type { DebugTabId } from "./debug-tabs";
import { DebugControlBar } from "./debug-control-bar";
import { DebugTabs } from "./debug-tabs";
import { DebugTimeline } from "./debug-timeline";
import { DumpExplorer } from "./dump-explorer";
import { AuditTab } from "./audit-tab";
import { ContextTab } from "./context-tab";
import { ManifestTab } from "./manifest-tab";
import { OverviewTab } from "./overview-tab";
import { ReplayTab } from "./replay-tab";
import { ToolCallsTab } from "./tool-calls-tab";

interface DebugModeProps {
  dumps: CoreDumpSummary[];
  selectedDumpId: string | null;
  selectedInfo: CoreDumpInfo | null;
  loading: boolean;
  capturePending: boolean;
  replayPendingId: string | null;
  activeReplaySourceDumpId: string | null;
  currentReplay: WorkspaceReplayDebugSnapshot | null;
  canCapture: boolean;
  onRefresh: () => void;
  onCapture: () => void;
  onSelectDump: (dumpId: string) => void;
  onReplayDump: (dumpId: string, branchLabel?: string | null) => Promise<void>;
}

export function DebugMode({
  dumps,
  selectedDumpId,
  selectedInfo,
  loading,
  capturePending,
  replayPendingId,
  activeReplaySourceDumpId,
  currentReplay,
  canCapture,
  onRefresh,
  onCapture,
  onSelectDump,
  onReplayDump,
}: DebugModeProps) {
  const [activeTab, setActiveTab] = useState<DebugTabId>("overview");
  const [branchLabel, setBranchLabel] = useState("");
  const manifest = useMemo(
    () => (selectedInfo ? parseCoreDumpManifest(selectedInfo.manifestJson) : null),
    [selectedInfo],
  );

  useEffect(() => {
    setBranchLabel("");
  }, [selectedDumpId]);

  async function handleReplay(dumpId: string) {
    await onReplayDump(dumpId, branchLabel.trim() || null);
  }

  async function copyToClipboard(value: string | null | undefined) {
    if (!value) {
      return;
    }
    await navigator.clipboard.writeText(value);
  }

  if (dumps.length === 0) {
    return (
      <section className="panel-surface flex flex-1 items-center justify-center p-8">
        <div className="max-w-lg text-center">
          <h2 className="text-2xl font-semibold tracking-tight text-slate-950">
            Debug mode is ready, but this session has no dumps yet.
          </h2>
          <p className="mt-3 text-sm leading-6 text-slate-500">
            Capture a first core dump to unlock manifest inspection, replay launch and forensic
            timeline analysis.
          </p>
          <button
            type="button"
            onClick={onCapture}
            disabled={!canCapture || capturePending}
            className="mt-6 inline-flex items-center gap-2 rounded-2xl bg-slate-950 px-5 py-3 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:bg-slate-300"
          >
            {capturePending ? "Capturing..." : "Capture First Dump"}
          </button>
        </div>
      </section>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
      <div className="grid items-start gap-4 pb-4 xl:grid-cols-[340px_minmax(0,1fr)]">
        <DumpExplorer
          dumps={dumps}
          selectedDumpId={selectedDumpId}
          loading={loading}
          capturePending={capturePending}
          activeReplaySourceDumpId={activeReplaySourceDumpId}
          canCapture={canCapture}
          onRefresh={onRefresh}
          onCapture={onCapture}
          onSelect={onSelectDump}
        />

        <div className="flex flex-col gap-4">
          <section className="panel-surface flex min-h-[34rem] flex-col">
            <DebugTabs activeTab={activeTab} onTabChange={setActiveTab} />
            <div className="flex-1 p-4">
              <DebugTabContent
                activeTab={activeTab}
                info={selectedInfo}
                manifest={manifest}
                branchLabel={branchLabel}
                replayPending={selectedDumpId !== null && replayPendingId === selectedDumpId}
                currentReplay={currentReplay}
                onBranchLabelChange={setBranchLabel}
                onReplay={() => {
                  if (selectedDumpId) {
                    void handleReplay(selectedDumpId);
                  }
                }}
              />
            </div>
            <DebugControlBar
              info={selectedInfo}
              onCopyDumpId={() => void copyToClipboard(selectedInfo?.dump.dumpId)}
              onCopyManifest={() => void copyToClipboard(selectedInfo?.manifestJson)}
            />
          </section>

          <DebugTimeline info={selectedInfo} manifest={manifest} />
        </div>
      </div>
    </div>
  );
}

function DebugTabContent({
  activeTab,
  info,
  manifest,
  branchLabel,
  replayPending,
  currentReplay,
  onBranchLabelChange,
  onReplay,
}: {
  activeTab: DebugTabId;
  info: CoreDumpInfo | null;
  manifest: ReturnType<typeof parseCoreDumpManifest> | null;
  branchLabel: string;
  replayPending: boolean;
  currentReplay: WorkspaceReplayDebugSnapshot | null;
  onBranchLabelChange: (value: string) => void;
  onReplay: () => void;
}) {
  switch (activeTab) {
    case "manifest":
      return <ManifestTab info={info} />;
    case "tool-calls":
      return <ToolCallsTab manifest={manifest} />;
    case "audit":
      return <AuditTab manifest={manifest} />;
    case "context":
      return <ContextTab manifest={manifest} />;
    case "replay":
      return (
        <ReplayTab
          info={info}
          branchLabel={branchLabel}
          replayPending={replayPending}
          currentReplay={currentReplay}
          onBranchLabelChange={onBranchLabelChange}
          onReplay={onReplay}
        />
      );
    case "overview":
    default:
      return <OverviewTab info={info} manifest={manifest} />;
  }
}
