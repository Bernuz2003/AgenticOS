import type { ReactNode } from "react";

interface RuntimeHealthCardProps {
  title: string;
  subtitle?: string;
  badges?: ReactNode;
  metrics?: Array<{ label: string; value: string }>;
  error?: string | null;
  children?: ReactNode;
}

export function RuntimeHealthCard({
  title,
  subtitle,
  badges,
  metrics,
  error,
  children,
}: RuntimeHealthCardProps) {
  return (
    <div className="rounded-2xl border border-slate-200 bg-white px-4 py-4">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <div className="text-sm font-semibold text-slate-900">{title}</div>
            {badges}
          </div>
          {subtitle && <div className="mt-1 text-xs text-slate-500">{subtitle}</div>}
          {error ? <div className="mt-1 text-xs text-rose-600">{error}</div> : null}
        </div>
        {metrics && metrics.length > 0 && (
          <div className="grid grid-cols-2 gap-3 text-right text-xs lg:grid-cols-3">
            {metrics.map((metric) => (
              <div key={metric.label}>
                <div className="text-slate-500">{metric.label}</div>
                <div className="font-medium text-slate-900">{metric.value}</div>
              </div>
            ))}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}
