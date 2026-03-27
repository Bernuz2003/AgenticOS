import type { OrchestrationStatus } from "../../lib/api";
import { IpcLog } from "../../components/diagnostics/ipc-log";

type WorkflowMessage = OrchestrationStatus["ipcMessages"][number];

interface WorkflowIpcPanelProps {
  messages: WorkflowMessage[];
}

export function WorkflowIpcPanel({ messages }: WorkflowIpcPanelProps) {
  return (
    <div className="mt-6 space-y-4">
      <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
          IPC / Message Bus
        </div>
        <div className="mt-2 text-sm text-slate-500">
          Structured process-to-process messages for this task and run.
        </div>
      </div>

      <IpcLog
        messages={messages}
        emptyMessage="No IPC messages captured for the selected task yet."
      />
    </div>
  );
}
