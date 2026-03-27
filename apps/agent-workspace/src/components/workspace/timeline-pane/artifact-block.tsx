interface ArtifactBlockProps {
  message: string;
}

export function ArtifactBlock({ message }: ArtifactBlockProps) {
  return (
    <div className="mx-auto max-w-3xl rounded-2xl border border-slate-200 bg-white px-5 py-4 text-sm text-slate-600 shadow-sm">
      {message}
    </div>
  );
}
