interface WorkflowRunStatusBadgeProps {
  status: string;
  label?: string;
  className?: string;
}

export function workflowRunStatusTone(status: string): string {
  switch (status) {
    case "running":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "completed":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "failed":
      return "border-rose-200 bg-rose-50 text-rose-700";
    case "skipped":
      return "border-amber-200 bg-amber-50 text-amber-700";
    default:
      return "border-slate-200 bg-slate-100 text-slate-700";
  }
}

export function WorkflowRunStatusBadge({
  status,
  label,
  className,
}: WorkflowRunStatusBadgeProps) {
  return (
    <span
      className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${workflowRunStatusTone(
        status,
      )} ${className ?? ""}`.trim()}
    >
      {label ?? status}
    </span>
  );
}
