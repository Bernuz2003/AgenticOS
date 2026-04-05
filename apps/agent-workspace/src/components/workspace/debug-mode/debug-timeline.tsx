import type { CoreDumpInfo } from "../../../lib/api";
import type { ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatTimestamp } from "../../../lib/workspace/format";

interface DebugTimelineProps {
  info: CoreDumpInfo | null;
  manifest: ParsedCoreDumpManifest | null;
}

type TimelineEventTone = "capture" | "checkpoint" | "tool" | "audit";

interface TimelineEvent {
  id: string;
  label: string;
  detail: string;
  timestamp: number | null;
  tone: TimelineEventTone;
}

export function DebugTimeline({ info, manifest }: DebugTimelineProps) {
  if (!info || !manifest) {
    return null;
  }

  const events: TimelineEvent[] = [
    {
      id: `capture-${info.dump.dumpId}`,
      label: info.manifest.capture.reason ?? info.dump.reason,
      detail: "capture",
      timestamp: info.dump.createdAtMs,
      tone: "capture" as TimelineEventTone,
    },
    ...manifest.debugCheckpoints.map((checkpoint) => ({
      id: checkpoint.id,
      label: checkpoint.boundaryKind ?? "checkpoint",
      detail: checkpoint.ordinal === null ? "checkpoint" : `step ${checkpoint.ordinal}`,
      timestamp: checkpoint.createdAtMs,
      tone: "checkpoint" as TimelineEventTone,
    })),
    ...manifest.toolInvocations.map((entry) => ({
      id: entry.id,
      label: entry.toolName,
      detail: entry.status ?? "tool call",
      timestamp: entry.createdAtMs,
      tone: "tool" as TimelineEventTone,
    })),
    ...manifest.auditEvents.map((entry, index) => ({
      id: `${entry.id}-${index}`,
      label: entry.title ?? entry.kind ?? "audit",
      detail: entry.category ?? "audit",
      timestamp: entry.recordedAtMs,
      tone: "audit" as TimelineEventTone,
    })),
  ]
    .sort((left, right) => (left.timestamp ?? 0) - (right.timestamp ?? 0))
    .slice(-12);

  if (events.length === 0) {
    return (
      <section className="panel-surface p-4 text-sm text-slate-500">
        No forensic event rail is available for this dump yet.
      </section>
    );
  }

  return (
    <section className="panel-surface p-4">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-950">Event Timeline</div>
          <div className="mt-1 text-sm text-slate-500">
            Temporal rail across checkpoints, tool calls, audit events and capture.
          </div>
        </div>
      </div>

      <div className="overflow-x-auto pb-2">
        <div className="flex min-w-max items-center gap-3">
          {events.map((event, index) => (
            <div key={event.id} className="flex items-center gap-3">
              <div className="min-w-[120px]">
                <div className={`h-3 w-3 rounded-full ${timelineTone(event.tone)}`} />
                <div className="mt-3 text-sm font-semibold text-slate-950">{event.label}</div>
                <div className="mt-1 text-xs uppercase tracking-[0.18em] text-slate-400">
                  {event.detail}
                </div>
                <div className="mt-2 text-xs text-slate-500">
                  {formatTimestamp(event.timestamp, {
                    hour: "2-digit",
                    minute: "2-digit",
                    second: "2-digit",
                  })}
                </div>
              </div>
              {index < events.length - 1 ? (
                <div className="h-px w-16 bg-slate-200" aria-hidden="true" />
              ) : null}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function timelineTone(tone: TimelineEventTone): string {
  switch (tone) {
    case "capture":
      return "bg-amber-500";
    case "checkpoint":
      return "bg-sky-500";
    case "audit":
      return "bg-rose-500";
    case "tool":
    default:
      return "bg-slate-900";
  }
}
