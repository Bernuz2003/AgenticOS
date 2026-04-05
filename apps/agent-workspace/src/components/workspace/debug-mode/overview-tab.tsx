import type { CoreDumpInfo } from "../../../lib/api";
import type { ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatBytes, formatWorkspaceValue } from "../../../lib/workspace/format";

interface OverviewTabProps {
  info: CoreDumpInfo | null;
  manifest: ParsedCoreDumpManifest | null;
}

export function OverviewTab({ info, manifest }: OverviewTabProps) {
  if (!info || !manifest) {
    return <EmptyState label="No dump selected." />;
  }

  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2 xl:grid-cols-3">
        <Panel title="Capture Summary">
          <Metric label="Reason" value={info.manifest.capture.reason ?? info.dump.reason} />
          <Metric label="Mode" value={info.manifest.capture.mode} />
          <Metric label="Fidelity" value={info.manifest.capture.fidelity ?? info.dump.fidelity} />
          <Metric label="Bytes" value={formatBytes(info.dump.bytes)} />
        </Panel>
        <Panel title="Target">
          <Metric label="Session" value={info.manifest.target.sessionId ?? info.dump.sessionId} />
          <Metric label="PID" value={info.manifest.target.pid ?? info.dump.pid} />
          <Metric label="Source" value={info.manifest.target.source} />
          <Metric label="State" value={info.manifest.target.state} />
        </Panel>
        <Panel title="Counts">
          <Metric label="Replay messages" value={info.manifest.counts.replayMessages} />
          <Metric label="Checkpoints" value={info.manifest.counts.debugCheckpoints} />
          <Metric label="Tool calls" value={info.manifest.counts.toolInvocations} />
          <Metric label="Audit events" value={info.manifest.counts.sessionAuditEvents} />
        </Panel>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <Panel title="Process Summary">
          <Metric label="Tool caller" value={info.manifest.process?.toolCaller} />
          <Metric label="Token count" value={info.manifest.process?.tokenCount} />
          <Metric label="Prompt chars" value={info.manifest.process?.promptChars} />
          <Metric
            label="Rendered prompt chars"
            value={info.manifest.process?.renderedPromptChars}
          />
          <Metric
            label="Termination reason"
            value={info.manifest.process?.terminationReason}
          />
        </Panel>
        <Panel title="Replay Readiness">
          <Metric label="Replay API" value="available" />
          <Metric label="Limitations" value={manifest.limitations.length} />
          <Metric label="Workspace entries" value={manifest.workspaceEntries.length} />
          <Metric label="Context segments" value={manifest.contextSegments.length} />
          <Metric label="Episodic segments" value={manifest.episodicSegments.length} />
        </Panel>
      </div>

      {manifest.limitations.length > 0 ? (
        <Panel title="Limitations">
          <ul className="space-y-2 text-sm text-amber-950">
            {manifest.limitations.map((entry) => (
              <li
                key={entry}
                className="rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3"
              >
                {entry}
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}
    </div>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
      <h3 className="text-sm font-semibold text-slate-950">{title}</h3>
      <div className="mt-4 space-y-3">{children}</div>
    </section>
  );
}

function Metric({
  label,
  value,
}: {
  label: string;
  value: number | string | null | undefined;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-b border-slate-200/70 pb-3 text-sm last:border-none last:pb-0">
      <span className="text-slate-500">{label}</span>
      <span className="text-right font-medium text-slate-950">
        {formatWorkspaceValue(value)}
      </span>
    </div>
  );
}

function EmptyState({ label }: { label: string }) {
  return <div className="text-sm text-slate-500">{label}</div>;
}
