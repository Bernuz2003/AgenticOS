import { Activity, Cloud, Cpu, Wrench } from "lucide-react";

import type { AuditEvent } from "../../../lib/api";

interface AuditCardProps {
  auditEvents: AuditEvent[];
  onOpenAudit: () => void;
}

export function AuditCard({ auditEvents, onOpenAudit }: AuditCardProps) {
  const diagnosticGroups = [
    {
      key: "tool",
      label: "Tools",
      icon: Wrench,
      events: auditEvents.filter((event) => event.category === "tool"),
    },
    {
      key: "remote",
      label: "Remote",
      icon: Cloud,
      events: auditEvents.filter((event) => event.category === "remote"),
    },
    {
      key: "process",
      label: "Process",
      icon: Activity,
      events: auditEvents.filter((event) => event.category === "process"),
    },
    {
      key: "runtime",
      label: "Runtime",
      icon: Cpu,
      events: auditEvents.filter(
        (event) => event.category === "runtime" || event.category === "admission",
      ),
    },
  ];
  const recentDiagnosticEvents = auditEvents.slice(0, 3);

  return (
    <section className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-xs font-bold uppercase tracking-wider text-slate-500">
            Diagnostics Overview
          </div>
          <div className="mt-1 text-sm font-semibold text-slate-900">
            {auditEvents.length} live audit events visible
          </div>
        </div>
        <button
          onClick={onOpenAudit}
          className="text-xs font-semibold text-indigo-600 hover:text-indigo-700"
        >
          Open full panel
        </button>
      </div>
      <div className="grid grid-cols-2 gap-3">
        {diagnosticGroups.map((group) => {
          const Icon = group.icon;
          const latest = group.events[0] ?? null;
          return (
            <div
              key={group.key}
              className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-500">
                  <Icon className="h-3.5 w-3.5 text-indigo-500" />
                  {group.label}
                </div>
                <span className="rounded-full border border-slate-200 bg-white px-2 py-0.5 text-[10px] font-bold text-slate-600">
                  {group.events.length}
                </span>
              </div>
              <div className="mt-2 text-sm font-semibold text-slate-900">
                {latest ? latest.title : "No events yet"}
              </div>
              <div className="mt-1 text-xs text-slate-500">
                {latest
                  ? `${latest.kind} at ${new Date(latest.recordedAtMs).toLocaleTimeString()}`
                  : "Awaiting diagnostics"}
              </div>
            </div>
          );
        })}
      </div>
      <div className="mt-4 space-y-2">
        {recentDiagnosticEvents.length === 0 ? (
          <div className="text-xs text-slate-500">Nessun evento tecnico recente.</div>
        ) : (
          recentDiagnosticEvents.map((event, index) => (
            <div
              key={`${event.recordedAtMs}-${event.category}-${index}`}
              className="rounded-xl border border-slate-100 bg-white px-3 py-3"
            >
              <div className="flex items-center justify-between gap-3">
                <div className="text-sm font-semibold text-slate-900">{event.title}</div>
                <div className="text-[10px] font-bold uppercase tracking-wider text-slate-500">
                  {event.category}
                </div>
              </div>
              <div className="mt-1 text-xs text-slate-500">
                {event.kind} at {new Date(event.recordedAtMs).toLocaleTimeString()}
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
