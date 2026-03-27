interface WorkflowRunProgressBarProps {
  value: number;
  className?: string;
}

export function WorkflowRunProgressBar({
  value,
  className,
}: WorkflowRunProgressBarProps) {
  const width = Math.max(0, Math.min(100, value));

  return (
    <div className={`h-2 overflow-hidden rounded-full bg-slate-200 ${className ?? ""}`.trim()}>
      <div
        className="h-full rounded-full bg-gradient-to-r from-indigo-500 to-sky-400"
        style={{ width: `${width}%` }}
      />
    </div>
  );
}
