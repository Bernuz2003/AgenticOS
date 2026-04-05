import type { ParsedCoreDumpManifest } from "../../../lib/workspace/core-dumps";
import { formatLatencyMs, formatTimestamp } from "../../../lib/workspace/format";

export function ToolCallsTab({ manifest }: { manifest: ParsedCoreDumpManifest | null }) {
  if (!manifest || manifest.toolInvocations.length === 0) {
    return <div className="text-sm text-slate-500">No tool invocations captured.</div>;
  }

  return (
    <div className="space-y-4">
      {manifest.toolInvocations.map((entry) => (
        <section
          key={entry.id}
          className="rounded-[24px] border border-slate-200 bg-slate-50 p-4"
        >
          <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div>
              <div className="text-sm font-semibold text-slate-950">{entry.toolName}</div>
              <div className="mt-1 text-xs text-slate-500">
                {formatTimestamp(entry.createdAtMs)} · {entry.status ?? "unknown status"}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              {entry.transport ? <Badge label={entry.transport} /> : null}
              {entry.caller ? <Badge label={entry.caller} /> : null}
              {entry.durationMs ? <Badge label={formatLatencyMs(entry.durationMs)} /> : null}
              {entry.kill ? <Badge label="killed" danger /> : null}
              {entry.errorKind ? <Badge label={entry.errorKind} danger /> : null}
            </div>
          </div>

          {entry.commandText ? (
            <CodeBlock label="Command">{entry.commandText}</CodeBlock>
          ) : null}
          {entry.inputPreview ? <CodeBlock label="Input">{entry.inputPreview}</CodeBlock> : null}
          {entry.outputPreview ? (
            <CodeBlock label="Output">{entry.outputPreview}</CodeBlock>
          ) : null}
          {entry.warnings.length > 0 ? (
            <div className="mt-4 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-950">
              <div className="font-semibold">Warnings</div>
              <div className="mt-2 space-y-1">
                {entry.warnings.map((warning) => (
                  <div key={warning}>{warning}</div>
                ))}
              </div>
            </div>
          ) : null}
        </section>
      ))}
    </div>
  );
}

function CodeBlock({
  label,
  children,
}: {
  label: string;
  children: string;
}) {
  return (
    <div className="mt-4 overflow-hidden rounded-[20px] border border-slate-200 bg-white">
      <div className="border-b border-slate-200 px-4 py-2 text-xs uppercase tracking-[0.18em] text-slate-400">
        {label}
      </div>
      <pre className="overflow-x-auto p-4 text-sm leading-6 text-slate-800">
        <code>{children}</code>
      </pre>
    </div>
  );
}

function Badge({ label, danger = false }: { label: string; danger?: boolean }) {
  return (
    <span
      className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.18em] ${
        danger
          ? "border-rose-200 bg-rose-50 text-rose-700"
          : "border-slate-200 bg-white text-slate-600"
      }`}
    >
      {label}
    </span>
  );
}
