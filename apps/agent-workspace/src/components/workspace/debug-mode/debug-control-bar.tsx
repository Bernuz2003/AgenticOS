import { Copy } from "lucide-react";

import type { CoreDumpInfo } from "../../../lib/api";

interface DebugControlBarProps {
  info: CoreDumpInfo | null;
  onCopyDumpId: () => void;
  onCopyManifest: () => void;
}

export function DebugControlBar({
  info,
  onCopyDumpId,
  onCopyManifest,
}: DebugControlBarProps) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-4">
      <div className="text-sm text-slate-500">
        {info ? `Selected dump: ${info.dump.dumpId}` : "Select a dump to activate debug controls."}
      </div>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={onCopyDumpId}
          disabled={!info}
          className="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:text-slate-950 disabled:cursor-not-allowed disabled:text-slate-400"
        >
          <Copy className="h-4 w-4" />
          Copy Dump ID
        </button>
        <button
          type="button"
          onClick={onCopyManifest}
          disabled={!info}
          className="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:text-slate-950 disabled:cursor-not-allowed disabled:text-slate-400"
        >
          <Copy className="h-4 w-4" />
          Copy Manifest
        </button>
      </div>
    </div>
  );
}
