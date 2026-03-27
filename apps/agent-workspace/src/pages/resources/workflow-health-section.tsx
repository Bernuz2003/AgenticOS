import { ArrowRight } from "lucide-react";
import { Link } from "react-router-dom";

import type { OrchestrationStatus } from "../../lib/api";
import type { LobbyOrchestrationSummary } from "../../store/sessions-store";
import { strategyLabel } from "../../lib/utils/formatting";
import {
  formatLatency,
  formatScore,
  taskStatusTone,
  workflowArtifactCount,
  workflowSessionCount,
} from "./format";

interface WorkflowHealthSectionProps {
  orchestrations: LobbyOrchestrationSummary[];
  workflowDetails: Record<number, OrchestrationStatus>;
  workflowError: string | null;
}

export function WorkflowHealthSection({
  orchestrations,
  workflowDetails,
  workflowError,
}: WorkflowHealthSectionProps) {
  return (
    <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="text-xs font-bold uppercase tracking-[0.2em] text-slate-400">
            Workflow Execution
          </div>
          <h2 className="mt-2 text-xl font-bold text-slate-900">
            Readable orchestration monitor with task attempts and artifacts
          </h2>
        </div>
        <div className="rounded-full border border-slate-200 bg-slate-50 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-slate-600">
          {orchestrations.length} workflows
        </div>
      </div>

      {workflowError ? (
        <div className="mt-5 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-700">
          {workflowError}
        </div>
      ) : null}

      {orchestrations.length === 0 ? (
        <div className="mt-6 rounded-3xl border border-dashed border-slate-200 bg-slate-50 px-6 py-12 text-center">
          <div className="text-lg font-semibold text-slate-800">No active workflows</div>
          <div className="mt-2 text-sm text-slate-500">
            Launch a workflow to observe task trees, attempts and artifacts here.
          </div>
        </div>
      ) : (
        <div className="mt-6 space-y-4">
          {orchestrations.map((workflow) => {
            const detail = workflowDetails[workflow.orchestrationId];
            return (
              <div
                key={workflow.orchestrationId}
                className="rounded-2xl border border-slate-200 bg-slate-50 p-5"
              >
                <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                  <div>
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="text-sm font-semibold text-slate-900">
                        Workflow {workflow.orchestrationId}
                      </div>
                      <span className="rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-indigo-700">
                        {workflow.policy}
                      </span>
                      {workflow.finished ? (
                        <span className="rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                          finished
                        </span>
                      ) : null}
                    </div>
                    <div className="mt-2 text-xs text-slate-500">
                      elapsed {workflow.elapsedLabel}
                      {detail
                        ? ` · ${workflowSessionCount(detail)} workspaces · ${workflowArtifactCount(detail)} artifacts`
                        : ""}
                    </div>
                  </div>
                  <Link
                    to="/workflows"
                    className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 hover:bg-slate-100"
                  >
                    Open Workflow Console
                    <ArrowRight className="h-4 w-4" />
                  </Link>
                </div>

                <div className="mt-4 grid grid-cols-5 gap-2 text-center text-xs">
                  <div className="rounded-xl border border-slate-200 bg-white px-3 py-2">
                    <div className="text-[10px] uppercase tracking-wider text-slate-400">
                      Total
                    </div>
                    <div className="mt-1 font-bold text-slate-900">{workflow.total}</div>
                  </div>
                  <div className="rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-2">
                    <div className="text-[10px] uppercase tracking-wider text-emerald-500">
                      Run
                    </div>
                    <div className="mt-1 font-bold text-emerald-700">{workflow.running}</div>
                  </div>
                  <div className="rounded-xl border border-sky-200 bg-sky-50 px-3 py-2">
                    <div className="text-[10px] uppercase tracking-wider text-sky-500">
                      Done
                    </div>
                    <div className="mt-1 font-bold text-sky-700">{workflow.completed}</div>
                  </div>
                  <div className="rounded-xl border border-amber-200 bg-amber-50 px-3 py-2">
                    <div className="text-[10px] uppercase tracking-wider text-amber-500">
                      Wait
                    </div>
                    <div className="mt-1 font-bold text-amber-700">{workflow.pending}</div>
                  </div>
                  <div className="rounded-xl border border-rose-200 bg-rose-50 px-3 py-2">
                    <div className="text-[10px] uppercase tracking-wider text-rose-500">
                      Fail
                    </div>
                    <div className="mt-1 font-bold text-rose-700">{workflow.failed}</div>
                  </div>
                </div>

                {!detail ? (
                  <div className="mt-4 text-sm text-slate-500">Loading workflow detail...</div>
                ) : (
                  <div className="mt-5 grid gap-3 xl:grid-cols-2">
                    {detail.tasks.map((task) => {
                      const liveAttempt = task.attempts.find(
                        (attempt) => attempt.attempt === task.currentAttempt,
                      );
                      const sessionId =
                        liveAttempt?.sessionId ??
                        task.attempts.find((attempt) => attempt.sessionId)?.sessionId ??
                        null;
                      return (
                        <div
                          key={`${detail.orchestrationId}-${task.task}`}
                          className="rounded-2xl border border-slate-200 bg-white p-4"
                        >
                          <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                            <div className="min-w-0 flex-1">
                              <div className="flex flex-wrap items-center gap-2">
                                <div className="text-sm font-semibold text-slate-900">
                                  {task.task}
                                </div>
                                <span
                                  className={`rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${taskStatusTone(
                                    task.status,
                                  )}`}
                                >
                                  {task.status}
                                </span>
                                {task.currentAttempt ? (
                                  <span className="rounded-full border border-slate-200 bg-slate-100 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-slate-600">
                                    attempt {task.currentAttempt}
                                  </span>
                                ) : null}
                              </div>
                              <div className="mt-2 text-xs text-slate-500">
                                role {task.role || "n/a"} · workload {task.workload || "default"}{" "}
                                · backend {task.backendClass || "auto"}
                              </div>
                            </div>
                            {sessionId ? (
                              <Link
                                to={`/workspace/${sessionId}`}
                                className="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs font-semibold text-slate-700 hover:bg-slate-100"
                              >
                                Open Task Workspace
                                <ArrowRight className="h-3.5 w-3.5" />
                              </Link>
                            ) : null}
                          </div>

                          <div className="mt-4 grid grid-cols-2 gap-3 text-xs">
                            <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                              <div className="text-slate-500">Dependencies</div>
                              <div className="mt-1 font-medium text-slate-900">
                                {task.deps.length > 0 ? task.deps.join(", ") : "root"}
                              </div>
                            </div>
                            <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                              <div className="text-slate-500">Attempts</div>
                              <div className="mt-1 font-medium text-slate-900">
                                {task.attempts.length}
                              </div>
                            </div>
                            <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                              <div className="text-slate-500">Input artifacts</div>
                              <div className="mt-1 font-medium text-slate-900">
                                {task.inputArtifacts.length}
                              </div>
                            </div>
                            <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-3">
                              <div className="text-slate-500">Output artifacts</div>
                              <div className="mt-1 font-medium text-slate-900">
                                {task.outputArtifacts.length}
                              </div>
                            </div>
                          </div>

                          {task.latestOutputPreview ? (
                            <div className="mt-4 rounded-xl border border-slate-200 bg-slate-50 px-4 py-3 text-xs leading-6 text-slate-600">
                              {task.latestOutputPreview}
                            </div>
                          ) : null}

                          {task.context ? (
                            <div className="mt-4 rounded-xl border border-indigo-200 bg-indigo-50/60 px-4 py-4">
                              <div className="text-[11px] font-bold uppercase tracking-[0.18em] text-indigo-700">
                                Semantic Retrieval
                              </div>
                              <div className="mt-3 grid grid-cols-2 gap-3 text-xs">
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Strategy</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {strategyLabel(task.context.contextStrategy)}
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Episodic Memory</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.episodicSegments} segments ·{" "}
                                    {task.context.episodicTokens} tokens
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Requests / Hits / Misses</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.contextRetrievalRequests} /{" "}
                                    {task.context.contextRetrievalHits} /{" "}
                                    {task.context.contextRetrievalMisses}
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Candidates / Selected</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.contextRetrievalCandidatesScored} /{" "}
                                    {task.context.contextRetrievalSegmentsSelected}
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Last Scan</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.lastRetrievalCandidatesScored} scored ·{" "}
                                    {task.context.lastRetrievalSegmentsSelected} kept
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Latency / Top Score</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {formatLatency(task.context.lastRetrievalLatencyMs)} ·{" "}
                                    {formatScore(task.context.lastRetrievalTopScore)}
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Top K / Candidate Limit</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.retrieveTopK} /{" "}
                                    {task.context.retrieveCandidateLimit}
                                  </div>
                                </div>
                                <div className="rounded-lg border border-indigo-100 bg-white/80 px-3 py-2">
                                  <div className="text-indigo-500">Min Score / Max Chars</div>
                                  <div className="mt-1 font-semibold text-slate-900">
                                    {task.context.retrieveMinScore.toFixed(2)} /{" "}
                                    {task.context.retrieveMaxSegmentChars}
                                  </div>
                                </div>
                              </div>
                            </div>
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
