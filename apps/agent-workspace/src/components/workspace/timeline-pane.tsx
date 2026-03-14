import { useEffect, useMemo, useRef, type FormEvent } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Brain, CheckCircle2, LoaderCircle, Sparkles, TerminalSquare, Wrench, XCircle, Send, User } from "lucide-react";
import type { TimelineSnapshot } from "../../lib/api";

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
  onComposerChange: (value: string) => void;
  onComposerSubmit: () => void | Promise<void>;
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
  onComposerChange,
  onComposerSubmit,
  onContinueOutput,
  onStopOutput,
}: TimelinePaneProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const timelineSignature = useMemo(() => {
    if (!timeline) {
      return "empty";
    }
    return timeline.items.map((item) => `${item.id}:${item.text.length}:${item.status}`).join("|");
  }, [timeline]);

  useEffect(() => {
    if (!scrollRef.current) {
      return;
    }
    scrollRef.current.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: timeline?.running ? "smooth" : "auto",
    });
  }, [timeline?.running, timelineSignature]);

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSend || composerLoading || !composerValue.trim()) {
      return;
    }
    void onComposerSubmit();
  }

  return (
    <section className="flex flex-col h-full bg-slate-50/50">
      <div ref={scrollRef} className="flex-1 overflow-y-auto p-6 space-y-8">
        {loading && (
          <div className="flex justify-center">
            <div className="inline-flex items-center gap-2 rounded-full bg-white px-4 py-2 text-sm font-medium text-slate-500 shadow-sm border border-slate-100">
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

        {!timeline || timeline.items.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-center opacity-60">
            <div className="w-16 h-16 bg-slate-200 rounded-full flex items-center justify-center mb-4">
              <TerminalSquare className="w-8 h-8 text-slate-400" />
            </div>
            <p className="text-slate-500 text-sm max-w-sm">
              Nessun evento disponibile per questo PID. Le nuove sessioni popolano questa Timeline in tempo reale.
            </p>
          </div>
        ) : (
          <div className="space-y-6 max-w-4xl mx-auto w-full">
            {timeline.items.map((item) => {
              if (item.kind === "user_message") {
                return (
                  <div key={item.id} className="flex gap-4 justify-end">
                    <div className="max-w-[80%] rounded-2xl rounded-tr-sm bg-indigo-600 px-5 py-3.5 text-sm leading-relaxed text-white shadow-sm">
                      {item.text}
                    </div>
                    <div className="w-8 h-8 rounded-full bg-indigo-100 flex items-center justify-center shrink-0 border border-indigo-200">
                      <User className="w-4 h-4 text-indigo-600" />
                    </div>
                  </div>
                );
              }

              if (item.kind === "assistant_message") {
                return (
                  <div key={item.id} className="flex gap-4">
                    <div className="w-8 h-8 rounded-full bg-emerald-100 flex items-center justify-center shrink-0 border border-emerald-200 mt-1">
                      <Sparkles className="w-4 h-4 text-emerald-600" />
                    </div>
                    <article className="max-w-[85%] rounded-2xl rounded-tl-sm bg-white px-6 py-5 text-[15px] leading-7 text-slate-700 shadow-sm border border-slate-100">
                      <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-400 mb-3">
                        AgenticOS
                        {item.status === "streaming" || item.status === "degraded-live" ? (
                          <span className="rounded-full bg-indigo-50 px-2 py-0.5 text-[10px] tracking-widest text-indigo-600 animate-pulse">
                            {item.status === "degraded-live" ? "FALLBACK" : "STREAMING"}
                          </span>
                        ) : null}
                      </div>
                      <div className="markdown-body prose prose-slate max-w-none prose-p:leading-relaxed prose-pre:bg-slate-900 prose-pre:text-slate-50">
                        <Markdown remarkPlugins={[remarkGfm]}>
                          {item.text || "In attesa dei primi chunk dal kernel..."}
                        </Markdown>
                      </div>
                    </article>
                  </div>
                );
              }

              if (item.kind === "thinking") {
                return (
                  <div key={item.id} className="flex gap-4 ml-12">
                    <details
                      className="max-w-[80%] rounded-2xl border border-slate-200 bg-slate-50/80 p-4 text-sm text-slate-700 shadow-sm group"
                      open={item.status === "streaming"}
                    >
                      <summary className="flex cursor-pointer list-none items-center gap-2 font-medium text-slate-500 hover:text-slate-800 transition-colors">
                        <Brain className="h-4 w-4" />
                        Thinking Process
                        {item.status === "streaming" && (
                          <span className="ml-2 rounded-full bg-slate-200 px-2 py-0.5 text-[10px] uppercase tracking-wider text-slate-600 animate-pulse">
                            Active
                          </span>
                        )}
                      </summary>
                      <div className="mt-4 whitespace-pre-wrap rounded-xl bg-white p-4 font-mono text-[13px] leading-relaxed text-slate-600 border border-slate-100">
                        {item.text}
                      </div>
                    </details>
                  </div>
                );
              }

              if (item.kind === "tool_call") {
                const renderedToolCall = item.text.trim().startsWith("TOOL:")
                  ? item.text
                  : `[[${item.text}]]`;
                const liveBadge = item.status === "streaming" ? "streaming" : item.status === "dispatching" ? "dispatching" : null;
                return (
                  <div key={item.id} className="flex gap-4 ml-12">
                     <div className="w-full max-w-[80%] rounded-2xl border border-indigo-100 bg-indigo-50/50 p-4 text-sm text-indigo-900 shadow-sm">
                        <div className="flex items-center gap-2 font-semibold text-indigo-700">
                          <Wrench className="h-4 w-4" />
                          Tool Execution
                          {liveBadge && (
                            <span className="rounded-full bg-indigo-200 px-2 py-0.5 text-[10px] uppercase tracking-wider text-indigo-800 animate-pulse">
                              {liveBadge}
                            </span>
                          )}
                        </div>
                        <div className="mt-3 rounded-xl bg-indigo-950 px-4 py-3 font-mono text-[12px] text-indigo-100 overflow-x-auto">
                          {renderedToolCall}
                        </div>
                     </div>
                  </div>
                );
              }

              if (item.kind === "tool_result") {
                const failed = item.status === "error";
                return (
                  <div key={item.id} className="flex gap-4 ml-12">
                    <article
                      className={`max-w-[80%] w-full rounded-2xl px-5 py-4 text-sm leading-relaxed shadow-sm border ${
                        failed
                          ? "border-rose-200 bg-rose-50 text-rose-900"
                          : "border-emerald-100 bg-emerald-50/50 text-emerald-900"
                      }`}
                    >
                      <div className={`flex items-center gap-2 text-xs font-bold uppercase tracking-wider mb-2 ${failed ? 'text-rose-600' : 'text-emerald-600'}`}>
                        {failed ? <XCircle className="h-4 w-4" /> : <CheckCircle2 className="h-4 w-4" />}
                        Result
                      </div>
                      <div className="markdown-body font-mono text-[13px] bg-white/60 p-3 rounded-xl border border-white/50">
                        <Markdown remarkPlugins={[remarkGfm]}>{item.text || "No output."}</Markdown>
                      </div>
                    </article>
                  </div>
                );
              }

              return (
                <div key={item.id} className="flex justify-center">
                  <div className="rounded-full border border-slate-200 bg-white px-4 py-2 text-xs font-medium text-slate-500 flex items-center gap-2 shadow-sm">
                    <TerminalSquare className="h-4 w-4" />
                    {item.text}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {awaitingContinuation && (
          <div className="max-w-xl mx-auto rounded-2xl border border-amber-200 bg-amber-50 px-6 py-5 text-sm text-amber-950 shadow-md">
            <div className="text-xs font-bold uppercase tracking-widest text-amber-600 mb-2">
              Risposta Interrotta
            </div>
            <p className="leading-relaxed mb-5">
              Questo messaggio ha raggiunto il limite di token del turno. Seleziona una delle opzioni sottostanti.
            </p>
            <div className="flex gap-3">
              <button
                type="button"
                onClick={() => void onContinueOutput()}
                disabled={turnActionLoading}
                className="rounded-xl flex-1 bg-amber-600 px-4 py-2.5 text-sm font-bold text-white transition hover:bg-amber-700 disabled:opacity-50"
              >
                {turnActionLoading ? "Attendere..." : "Continua Output"}
              </button>
              <button
                type="button"
                onClick={() => void onStopOutput()}
                disabled={turnActionLoading}
                className="rounded-xl flex-1 border border-amber-300 bg-white px-4 py-2.5 text-sm font-bold text-amber-700 transition hover:bg-amber-100 disabled:opacity-50"
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

      <div className="p-4 bg-white border-t border-slate-200 shrink-0">
        <form onSubmit={handleSubmit} className="max-w-4xl mx-auto relative flex items-end gap-2">
          <textarea
            value={composerValue}
            onChange={(event) => onComposerChange(event.target.value)}
            placeholder={
              !canSend 
                ? "Il composer si abilita quando il processo entra in WaitingForInput..."
                : "Invia un messaggio o un prompt all'agente..."
            }
            disabled={!canSend || composerLoading || turnActionLoading}
            className="w-full max-h-60 min-h-[56px] resize-y rounded-2xl border border-slate-300 bg-white px-5 py-4 pr-16 text-[15px] leading-relaxed text-slate-900 shadow-sm outline-none transition-all focus:border-indigo-500 focus:ring-4 focus:ring-indigo-500/10 disabled:bg-slate-50 disabled:text-slate-500"
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSubmit(e as any);
              }
            }}
          />
          <button
            type="submit"
            disabled={!canSend || composerLoading || !composerValue.trim()}
            className="absolute right-2 bottom-2 rounded-xl bg-indigo-600 p-3 text-white transition-all hover:bg-indigo-700 hover:scale-105 active:scale-95 disabled:pointer-events-none disabled:opacity-50 disabled:bg-slate-300"
            title="Send Message"
          >
            {composerLoading ? <LoaderCircle className="w-5 h-5 animate-spin" /> : <Send className="w-5 h-5" />}
          </button>
        </form>
        {composerError && (
          <div className="max-w-4xl mx-auto mt-3 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
            {composerError}
          </div>
        )}
      </div>
    </section>
  );
}
