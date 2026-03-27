function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface WorkflowArtifactPreviewProps {
  title: string;
  subtitle: string;
  bytes: number;
  body: string | null;
}

export function WorkflowArtifactPreview({
  title,
  subtitle,
  bytes,
  body,
}: WorkflowArtifactPreviewProps) {
  return (
    <article className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-slate-900">{title}</div>
          <div className="mt-1 text-xs text-slate-500">{subtitle}</div>
        </div>
        <div className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[11px] font-semibold text-slate-600">
          {formatBytes(bytes)}
        </div>
      </div>
      <div className="mt-3 whitespace-pre-wrap break-words text-sm leading-6 text-slate-700">
        {body || "Empty artifact"}
      </div>
    </article>
  );
}
