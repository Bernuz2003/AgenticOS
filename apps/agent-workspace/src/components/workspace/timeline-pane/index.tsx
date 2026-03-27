import { useEffect, useMemo, useRef } from "react";
import { LoaderCircle } from "lucide-react";
import type { HumanInputRequest, TimelineSnapshot } from "../../../lib/api";
import { filterRenderableTimelineItems } from "../../../lib/timeline/mapping";
import { hiddenTimelineItemCount } from "../../../lib/timeline/grouping";
import { WorkspaceComposer } from "../composer";
import { ArtifactBlock } from "./artifact-block";
import { composerPlaceholder, timelineSignature } from "./markers";
import { MessageList } from "./message-list";

interface TimelinePaneProps {
  timeline: TimelineSnapshot | null;
  loading: boolean;
  error: string | null;
  awaitingContinuation: boolean;
  composerValue: string;
  composerLoading: boolean;
  composerError: string | null;
  turnActionLoading: boolean;
  turnActionError: string | null;
  canSend: boolean;
  canSendText: boolean;
  humanRequest: HumanInputRequest | null;
  onComposerChange: (value: string) => void;
  onComposerSubmit: () => void | Promise<void>;
  onHumanChoice: (choice: string) => void | Promise<void>;
  onContinueOutput: () => void | Promise<void>;
  onStopOutput: () => void | Promise<void>;
}

export function TimelinePane({
  timeline,
  loading,
  error,
  awaitingContinuation,
  composerValue,
  composerLoading,
  composerError,
  turnActionLoading,
  turnActionError,
  canSend,
  canSendText,
  humanRequest,
  onComposerChange,
  onComposerSubmit,
  onHumanChoice,
  onContinueOutput,
  onStopOutput,
}: TimelinePaneProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const renderedItems = useMemo(
    () => (timeline ? filterRenderableTimelineItems(timeline.items) : []),
    [timeline],
  );
  const hiddenDiagnosticsCount = hiddenTimelineItemCount(
    timeline?.items,
    renderedItems.length,
  );
  const renderedTimelineSignature = useMemo(
    () => timelineSignature(renderedItems),
    [renderedItems],
  );

  useEffect(() => {
    if (!scrollRef.current) {
      return;
    }
    scrollRef.current.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: timeline?.running ? "smooth" : "auto",
    });
  }, [renderedTimelineSignature, timeline?.running]);

  const humanChoiceLoading = composerLoading || turnActionLoading;
  const placeholder = composerPlaceholder(humanRequest, canSend);

  return (
    <section className="flex h-full flex-col bg-slate-50/50">
      <div ref={scrollRef} className="flex-1 space-y-8 overflow-y-auto p-6">
        {loading && (
          <div className="flex justify-center">
            <div className="inline-flex items-center gap-2 rounded-full border border-slate-100 bg-white px-4 py-2 text-sm font-medium text-slate-500 shadow-sm">
              <LoaderCircle className="h-4 w-4 animate-spin text-indigo-500" />
              Syncing EXEC timeline...
            </div>
          </div>
        )}

        {timeline?.fallbackNotice && (
          <div className="rounded-2xl border border-amber-200 bg-amber-50 px-5 py-4 text-sm text-amber-900 mx-auto max-w-3xl">
            {timeline.fallbackNotice}
          </div>
        )}

        {error && (
          <div className="rounded-2xl border border-rose-200 bg-rose-50 px-5 py-4 text-sm text-rose-800 mx-auto max-w-3xl">
            {error}
          </div>
        )}

        {hiddenDiagnosticsCount > 0 && (
          <ArtifactBlock
            message={
              `${hiddenDiagnosticsCount} eventi tecnici sono stati spostati nel pannello diagnostico per mantenere la chat pulita.`
            }
          />
        )}

        {humanRequest && (
          <div className="mx-auto max-w-3xl rounded-2xl border border-sky-200 bg-sky-50 px-6 py-5 text-sm text-sky-950 shadow-sm">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs font-bold uppercase tracking-widest text-sky-600">
                  {humanRequest.kind === "approval"
                    ? "Approval Required"
                    : "Human Input Requested"}
                </div>
                <div className="mt-2 text-base font-semibold text-slate-900">
                  {humanRequest.question}
                </div>
              </div>
              <div className="rounded-full border border-sky-200 bg-white px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-sky-700">
                Waiting
              </div>
            </div>
            {humanRequest.details && (
              <div className="mt-3 rounded-xl border border-sky-100 bg-white/70 px-4 py-3 text-sm leading-relaxed text-slate-700">
                {humanRequest.details}
              </div>
            )}
            {humanRequest.choices.length > 0 && (
              <div className="mt-4 flex flex-wrap gap-2">
                {humanRequest.choices.map((choice) => (
                  <button
                    key={choice}
                    type="button"
                    onClick={() => void onHumanChoice(choice)}
                    disabled={humanChoiceLoading}
                    className="rounded-xl border border-sky-200 bg-white px-4 py-2 text-sm font-semibold text-sky-800 transition hover:border-sky-300 hover:bg-sky-100 disabled:opacity-50"
                  >
                    {choice}
                  </button>
                ))}
              </div>
            )}
            <div className="mt-4 text-xs text-sky-900/80">
              {humanRequest.allowFreeText
                ? "Puoi rispondere con testo libero oppure usare una delle opzioni rapide."
                : "Questo step e' bloccato su risposta strutturata. Usa una delle opzioni rapide per riprendere il processo."}
            </div>
          </div>
        )}

        {!timeline ? (
          <MessageList items={[]} />
        ) : (
          <MessageList items={renderedItems} />
        )}

        {awaitingContinuation && (
          <div className="mx-auto max-w-xl rounded-2xl border border-amber-200 bg-amber-50 px-6 py-5 text-sm text-amber-950 shadow-md">
            <div className="mb-2 text-xs font-bold uppercase tracking-widest text-amber-600">
              Risposta Interrotta
            </div>
            <p className="mb-5 leading-relaxed">
              Questo messaggio ha raggiunto il limite di token del turno. Seleziona una
              delle opzioni sottostanti.
            </p>
            <div className="flex gap-3">
              <button
                type="button"
                onClick={() => void onContinueOutput()}
                disabled={turnActionLoading}
                className="flex-1 rounded-xl bg-amber-600 px-4 py-2.5 text-sm font-bold text-white transition hover:bg-amber-700 disabled:opacity-50"
              >
                {turnActionLoading ? "Attendere..." : "Continua Output"}
              </button>
              <button
                type="button"
                onClick={() => void onStopOutput()}
                disabled={turnActionLoading}
                className="flex-1 rounded-xl border border-amber-300 bg-white px-4 py-2.5 text-sm font-bold text-amber-700 transition hover:bg-amber-100 disabled:opacity-50"
              >
                Interrompi
              </button>
            </div>
            {turnActionError && (
              <div className="mt-4 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
                {turnActionError}
              </div>
            )}
          </div>
        )}
      </div>

      <WorkspaceComposer
        value={composerValue}
        placeholder={placeholder}
        disabled={!canSendText || composerLoading || turnActionLoading}
        loading={composerLoading}
        error={composerError}
        onChange={onComposerChange}
        onSubmit={() => {
          if (canSendText && !composerLoading && composerValue.trim()) {
            void onComposerSubmit();
          }
        }}
      />
    </section>
  );
}
