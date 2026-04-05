import { useMemo } from "react";

import { PreviewRecordList } from "../../ui/preview-record-list";
import type { ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatTimestamp } from "../../../lib/workspace/format";

export function AuditTab({ manifest }: { manifest: ParsedCoreDumpManifest | null }) {
  const entries = useMemo(
    () =>
      manifest
        ? [...manifest.auditEvents].sort(
            (left, right) =>
              (right.recordedAtMs ?? Number.MIN_SAFE_INTEGER) -
              (left.recordedAtMs ?? Number.MIN_SAFE_INTEGER),
          )
        : [],
    [manifest],
  );

  return (
    <PreviewRecordList
      items={entries}
      previewLimit={6}
      emptyState={
        <div className="text-sm text-slate-500">No audit events stored in this dump.</div>
      }
      getKey={(entry) => entry.id}
      renderItem={(entry) => <AuditEntryCard entry={entry} />}
      modalTitle="Session Audit"
      modalDescription="Audit completo memorizzato nel core dump selezionato."
    />
  );
}

function AuditEntryCard({
  entry,
}: {
  entry: ParsedCoreDumpManifest["auditEvents"][number];
}) {
  return (
    <section className="rounded-[24px] border border-slate-200 bg-slate-50 p-4">
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
        <div className="mt-4 max-h-72 overflow-auto whitespace-pre-wrap rounded-[20px] border border-slate-200 bg-white px-4 py-3 text-sm leading-6 text-slate-800">
          {entry.detail}
        </div>
      ) : null}
    </section>
  );
}

function Badge({ label }: { label: string }) {
  return (
    <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.18em] text-slate-600">
      {label}
    </span>
  );
}
