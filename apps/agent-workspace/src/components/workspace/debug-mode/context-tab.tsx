import type { ParsedCoreDumpContextSegment, ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatBytes, formatWorkspaceValue } from "../../../lib/workspace/format";

export function ContextTab({ manifest }: { manifest: ParsedCoreDumpManifest | null }) {
  if (!manifest) {
    return <div className="text-sm text-slate-500">No context snapshot loaded.</div>;
  }

  return (
    <div className="space-y-4">
      <div className="grid gap-4 xl:grid-cols-3">
        <SummaryCard label="Replay messages" value={manifest.replayMessages.length} />
        <SummaryCard label="Context segments" value={manifest.contextSegments.length} />
        <SummaryCard label="Workspace entries" value={manifest.workspaceEntries.length} />
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <SegmentPanel title="Context Segments" segments={manifest.contextSegments} />
        <SegmentPanel title="Episodic Segments" segments={manifest.episodicSegments} />
      </div>

      <section className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
        <h3 className="text-sm font-semibold text-slate-950">Workspace Snapshot</h3>
        <div className="mt-4 space-y-3">
          {manifest.workspaceEntries.length === 0 ? (
            <div className="text-sm text-slate-500">No workspace entries captured.</div>
          ) : (
            manifest.workspaceEntries.map((entry) => (
              <div
                key={entry.id}
                className="rounded-[20px] border border-slate-200 bg-white px-4 py-3"
              >
                <div className="text-sm font-semibold text-slate-950">
                  {entry.path ?? "unknown path"}
                </div>
                <div className="mt-1 text-xs text-slate-500">
                  {formatWorkspaceValue(entry.kind)} · {formatBytes(entry.bytes)}
                </div>
              </div>
            ))
          )}
        </div>
      </section>
    </div>
  );
}

function SummaryCard({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
      <div className="text-xs uppercase tracking-[0.18em] text-slate-400">{label}</div>
      <div className="mt-2 text-2xl font-semibold tracking-tight text-slate-950">{value}</div>
    </div>
  );
}

function SegmentPanel({
  title,
  segments,
}: {
  title: string;
  segments: ParsedCoreDumpContextSegment[];
}) {
  return (
    <section className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
      <h3 className="text-sm font-semibold text-slate-950">{title}</h3>
      <div className="mt-4 space-y-3">
        {segments.length === 0 ? (
          <div className="text-sm text-slate-500">No segments captured.</div>
        ) : (
          segments.map((segment) => (
            <div
              key={segment.id}
              className="rounded-[20px] border border-slate-200 bg-white px-4 py-3"
            >
              <div className="flex items-center justify-between gap-3">
                <div className="text-sm font-semibold text-slate-950">
                  {segment.kind ?? "segment"}
                </div>
                <div className="text-xs text-slate-500">
                  {segment.tokens !== null ? `${segment.tokens} tok` : "tokens n/a"}
                </div>
              </div>
              <div className="mt-2 whitespace-pre-wrap text-sm leading-6 text-slate-700">
                {segment.text ?? "No text available."}
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
