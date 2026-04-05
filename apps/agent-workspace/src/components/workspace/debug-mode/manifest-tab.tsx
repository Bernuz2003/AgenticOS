import type { CoreDumpInfo } from "../../../lib/api";

export function ManifestTab({ info }: { info: CoreDumpInfo | null }) {
  if (!info) {
    return <div className="text-sm text-slate-500">No manifest loaded.</div>;
  }

  return (
    <div className="overflow-hidden rounded-[24px] border border-slate-200 bg-slate-950">
      <pre className="max-h-[520px] overflow-auto p-5 text-sm leading-6 text-slate-100">
        <code>{info.manifestJson}</code>
      </pre>
    </div>
  );
}
