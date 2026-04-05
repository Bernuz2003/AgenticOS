import type { ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatTimestamp } from "../../../lib/workspace/format";

export function AuditTab({ manifest }: { manifest: ParsedCoreDumpManifest | null }) {
  if (!manifest || manifest.auditEvents.length === 0) {
    return <div className="text-sm text-slate-500">No audit events stored in this dump.</div>;
  }

  return (
    <div className="space-y-4">
      {manifest.auditEvents.map((entry) => (
        <section
          key={entry.id}
          className="rounded-[24px] border border-slate-200 bg-slate-50 p-4"
        >
          <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div>
              <div className="text-sm font-semibold text-slate-950">
                {entry.title ?? entry.kind ?? "Audit event"}
              </div>
              <div className="mt-1 text-xs text-slate-500">
                {formatTimestamp(entry.recordedAtMs)} · {entry.category ?? "audit"}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              {entry.kind ? <Badge label={entry.kind} /> : null}
              {entry.pid !== null ? <Badge label={`PID ${entry.pid}`} /> : null}
            </div>
          </div>
          {entry.detail ? (
            <div className="mt-4 whitespace-pre-wrap rounded-[20px] border border-slate-200 bg-white px-4 py-3 text-sm leading-6 text-slate-800">
              {entry.detail}
            </div>
          ) : null}
        </section>
      ))}
    </div>
  );
}

function Badge({ label }: { label: string }) {
  return (
    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.18em] text-slate-600">
      {label}
    </span>
  );
}
