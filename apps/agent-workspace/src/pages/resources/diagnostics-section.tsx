import type { AuditEvent } from "../../lib/api";
import { categoryIcon, diagnosticTone, formatRelative } from "./format";

interface DiagnosticsSectionProps {
  events: AuditEvent[];
  categoryOptions: string[];
  categoryCounts: Map<string, number>;
  diagnosticHighlights: Array<{
    category: string;
    count: number;
    latest: AuditEvent | null;
  }>;
  selectedCategory: string;
  onCategoryChange: (category: string) => void;
}

export function DiagnosticsSection({
  events,
  categoryOptions,
  categoryCounts,
  diagnosticHighlights,
  selectedCategory,
  onCategoryChange,
}: DiagnosticsSectionProps) {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Diagnostics Console
          </div>
          <h2 className="mt-2 text-xl font-bold text-slate-900">
            Tool plane, remote requests, runtime transitions and errors
          </h2>
          <p className="mt-2 text-sm text-slate-500">
            Live audit events remain separate from the chat timeline and are
            grouped here for operator debugging.
          </p>
        </div>
        <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
          {events.length} events
        </div>
      </div>

      <div className="mt-6 grid gap-3 md:grid-cols-4">
        {diagnosticHighlights.map(({ category, count, latest }) => {
          const Icon = categoryIcon(category);
          return (
            <div
              key={category}
              className={`rounded-2xl border px-4 py-4 ${diagnosticTone(category)}`}
            >
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider">
                  <Icon className="h-4 w-4" />
                  {category}
                </div>
                <div className="rounded-full border border-current/20 bg-white/60 px-2 py-0.5 text-[10px] font-bold">
                  {count}
                </div>
              </div>
              <div className="mt-3 text-sm font-semibold">
                {latest ? latest.title : "No events yet"}
              </div>
              <div className="mt-1 text-xs opacity-80">
                {latest ? formatRelative(latest.recordedAtMs) : "Awaiting signal"}
              </div>
            </div>
          );
        })}
      </div>

      <div className="mt-6 flex flex-wrap gap-2">
        {categoryOptions.map((category) => {
          const active = selectedCategory === category;
          const count =
            category === "all" ? events.length : (categoryCounts.get(category) ?? 0);
          return (
            <button
              key={category}
              onClick={() => onCategoryChange(category)}
              className={`rounded-full border px-3 py-1.5 text-[11px] font-bold uppercase tracking-wider transition-colors ${
                active
                  ? "border-indigo-200 bg-indigo-50 text-indigo-700"
                  : "border-slate-200 bg-white text-slate-500 hover:border-slate-300 hover:text-slate-700"
              }`}
            >
              {category} ({count})
            </button>
          );
        })}
      </div>

      {events.length === 0 ? (
        <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center text-sm text-slate-500">
          No diagnostics recorded for the selected filter.
        </div>
      ) : (
        <div className="mt-6 space-y-3">
          {events.slice(0, 20).map((event, index) => (
            <DiagnosticEventCard
              key={`${event.recordedAtMs}-${event.category}-${index}`}
              event={event}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function DiagnosticEventCard({ event }: { event: AuditEvent }) {
  const Icon = categoryIcon(event.category);

  return (
    <div className="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4">
      <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span
              className={`inline-flex items-center gap-2 rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${diagnosticTone(
                event.category,
              )}`}
            >
              <Icon className="h-3.5 w-3.5" />
              {event.category}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
              {event.kind}
            </span>
            <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-500">
              {new Date(event.recordedAtMs).toLocaleString()}
            </span>
          </div>
          <div className="mt-3 text-sm font-semibold text-slate-900">{event.title}</div>
          <div className="mt-2 whitespace-pre-wrap break-words rounded-xl border border-slate-200 bg-white px-4 py-3 font-mono text-xs leading-6 text-slate-600">
            {event.detail}
          </div>
        </div>
        <div className="grid grid-cols-1 gap-2 text-xs xl:w-56">
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">session_id</div>
            <div className="mt-1 font-medium text-slate-900">{event.sessionId || "n/a"}</div>
          </div>
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">pid</div>
            <div className="mt-1 font-medium text-slate-900">{event.pid ?? "n/a"}</div>
          </div>
          <div className="rounded-xl border border-slate-200 bg-white px-3 py-3">
            <div className="text-slate-500">runtime_id</div>
            <div className="mt-1 font-medium text-slate-900">{event.runtimeId || "n/a"}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
