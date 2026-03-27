import type { OrchestrationStatus } from "../../lib/api";
import { WorkflowArtifactPreview } from "../../components/workflows/run/artifact-preview";

type WorkflowTask = OrchestrationStatus["tasks"][number];

interface WorkflowArtifactsPanelProps {
  task: WorkflowTask;
}

export function WorkflowArtifactsPanel({ task }: WorkflowArtifactsPanelProps) {
  return (
    <div className="mt-6 space-y-4">
      <div className="rounded-2xl border border-slate-200 bg-slate-50 p-4">
        <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
          Input Artifacts
        </div>
        {task.inputArtifacts.length === 0 ? (
          <div className="mt-3 text-sm text-slate-500">No upstream artifacts.</div>
        ) : (
          <div className="mt-3 flex flex-wrap gap-2">
            {task.inputArtifacts.map((artifact) => (
              <span
                key={artifact.artifactId}
                className="rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1 text-[11px] font-semibold text-emerald-700"
              >
                {artifact.task} #{artifact.attempt} · {artifact.label}
              </span>
            ))}
          </div>
        )}
      </div>

      <div className="space-y-3">
        {task.outputArtifacts.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-5 py-8 text-sm text-slate-500">
            No persisted artifacts yet.
          </div>
        ) : (
          task.outputArtifacts.map((artifact) => (
            <WorkflowArtifactPreview
              key={artifact.artifactId}
              title={artifact.label}
              subtitle={`${artifact.kind} · attempt ${artifact.attempt}`}
              bytes={artifact.bytes}
              body={artifact.content || artifact.preview || "Empty artifact"}
            />
          ))
        )}
      </div>
    </div>
  );
}
