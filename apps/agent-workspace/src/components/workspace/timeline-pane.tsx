import { useEffect, useMemo, useRef, type FormEvent } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Brain, CheckCircle2, LoaderCircle, Sparkles, TerminalSquare, Wrench, XCircle } from "lucide-react";
import type { TimelineSnapshot } from "../../lib/api";
import type { AgentSessionSummary } from "../../store/sessions-store";

interface TimelinePaneProps {
  session: AgentSessionSummary;
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
  session,
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
    <section className="panel-surface flex min-h-[680px] flex-col gap-6 p-6">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.28em] text-slate-500">
            Timeline
          </p>
          <h2 className="mt-2 text-2xl font-bold tracking-tight text-slate-950">
            Conversazione e loop operativo
          </h2>
        </div>
        <span className="rounded-full bg-slate-950 px-3 py-1 text-xs font-semibold uppercase tracking-[0.22em] text-white">
          {session.status}
        </span>
      </div>

      <div ref={scrollRef} className="flex flex-1 flex-col gap-5 overflow-auto pr-1">
        {loading ? (
          <div className="inline-flex items-center gap-3 rounded-[24px] border border-slate-200 bg-white px-5 py-4 text-sm text-slate-600">
            <LoaderCircle className="h-4 w-4 animate-spin" />
            Syncing EXEC timeline...
          </div>
        ) : null}

        {timeline?.fallbackNotice ? (
          <div className="rounded-[24px] border border-amber-200 bg-amber-50 px-5 py-4 text-sm text-amber-900">
            {timeline.fallbackNotice}
          </div>
        ) : null}

        {error ? (
          <div className="rounded-[24px] border border-rose-200 bg-rose-50 px-5 py-4 text-sm text-rose-800">
            {error}
          </div>
        ) : null}

        {!timeline || timeline.items.length === 0 ? (
          <div className="rounded-[24px] border border-slate-200 bg-white px-5 py-8 text-sm text-slate-600">
            Nessun evento `EXEC` disponibile per questo PID. Le nuove sessioni avviate dalla Lobby popolano questa Timeline in tempo reale.
          </div>
        ) : (
          timeline.items.map((item) => {
            if (item.kind === "user_message") {
              return (
                <div
                  key={item.id}
                  className="ml-auto max-w-[78%] rounded-[24px] bg-slate-950 px-5 py-4 text-sm leading-6 text-white"
                >
                  {item.text}
                </div>
              );
            }

            if (item.kind === "assistant_message") {
              return (
                <article
                  key={item.id}
                  className="max-w-[86%] rounded-[24px] bg-white px-5 py-5 text-sm leading-7 text-slate-700 shadow-sm"
                >
                  <div className="flex items-center gap-3 text-xs font-semibold uppercase tracking-[0.24em] text-slate-500">
                    <Sparkles className="h-4 w-4" />
                    Assistant
                    {item.status === "streaming" || item.status === "degraded-live" ? (
                      <span className="rounded-full bg-emerald-50 px-2 py-1 text-[10px] tracking-[0.18em] text-emerald-700">
                        {item.status === "degraded-live" ? "fallback" : "streaming"}
                      </span>
                    ) : null}
                  </div>
                  <div className="markdown-body mt-3">
                    <Markdown remarkPlugins={[remarkGfm]}>
                      {item.text || "In attesa dei primi chunk dal kernel..."}
                    </Markdown>
                  </div>
                </article>
              );
            }

            if (item.kind === "thinking") {
              return (
                <details
                  key={item.id}
                  className="max-w-[86%] rounded-[24px] border border-sky-200 bg-sky-50/80 p-5 text-sm text-sky-950"
                  open={item.status === "streaming"}
                >
                  <summary className="flex cursor-pointer list-none items-center gap-3 font-semibold">
                    <Brain className="h-5 w-5" />
                    Thinking
                    {item.status === "streaming" ? (
                      <span className="rounded-full bg-sky-100 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-sky-700">
                        streaming
                      </span>
                    ) : null}
                  </summary>
                  <div className="mt-4 whitespace-pre-wrap rounded-2xl bg-white/70 p-4 font-mono text-[13px] leading-6 text-slate-700">
                    {item.text}
                  </div>
                </details>
              );
            }

            if (item.kind === "tool_call") {
              return (
                <div
                  key={item.id}
                  className="rounded-[24px] border border-cyan-200 bg-cyan-50/90 p-5 text-sm text-cyan-950"
                >
                  <div className="flex items-center gap-3 font-semibold">
                    <Wrench className="h-5 w-5" />
                    Tool Call
                    {item.status === "streaming" ? (
                      <span className="rounded-full bg-cyan-100 px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-cyan-700">
                        streaming
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-3 rounded-2xl bg-slate-950 px-4 py-3 font-mono text-[13px] text-cyan-50">
                    [[{item.text}]]
                  </div>
                </div>
              );
            }

            if (item.kind === "tool_result") {
              const failed = item.status === "error";
              return (
                <article
                  key={item.id}
                  className={`max-w-[86%] rounded-[24px] px-5 py-5 text-sm leading-7 shadow-sm ${
                    failed
                      ? "border border-rose-200 bg-rose-50 text-rose-900"
                      : "border border-emerald-200 bg-emerald-50 text-emerald-950"
                  }`}
                >
                  <div className="flex items-center gap-3 text-xs font-semibold uppercase tracking-[0.24em]">
                    {failed ? <XCircle className="h-4 w-4" /> : <CheckCircle2 className="h-4 w-4" />}
                    Tool Result
                  </div>
                  <div className="markdown-body mt-3">
                    <Markdown remarkPlugins={[remarkGfm]}>{item.text}</Markdown>
                  </div>
                </article>
              );
            }

            return (
              <div
                key={item.id}
                className="rounded-[24px] border border-cyan-200 bg-cyan-50/90 p-5 text-sm text-cyan-950"
              >
                <div className="flex items-center gap-3 font-semibold">
                  <TerminalSquare className="h-5 w-5" />
                  System Event
                </div>
                <div className="mt-3 rounded-2xl bg-slate-950 px-4 py-3 font-mono text-[13px] text-cyan-50">
                  {item.text}
                </div>
              </div>
            );
          })
        )}

        {timeline?.running ? (
          <div className="text-xs uppercase tracking-[0.22em] text-slate-500">
            {timeline.source === "status_fallback" ? "Fallback STATUS" : "Stream live"} per PID {timeline.pid} · workload {timeline.workload || session.contextStrategy}
          </div>
        ) : null}

        {awaitingContinuation ? (
          <div className="max-w-[86%] rounded-[24px] border border-amber-200 bg-amber-50 px-5 py-5 text-sm text-amber-950 shadow-sm">
            <div className="text-xs font-semibold uppercase tracking-[0.24em] text-amber-700">
              Risposta interrotta
            </div>
            <p className="mt-2 leading-6">
              Questo messaggio ha raggiunto il limite di token del turno. Continuare la risposta?
            </p>
            <div className="mt-4 flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => void onContinueOutput()}
                disabled={turnActionLoading}
                className="rounded-full bg-slate-950 px-4 py-2 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:bg-slate-300"
              >
                {turnActionLoading ? "Attendere..." : "Si"}
              </button>
              <button
                type="button"
                onClick={() => void onStopOutput()}
                disabled={turnActionLoading}
                className="rounded-full border border-slate-300 bg-white px-4 py-2 text-sm font-semibold text-slate-700 transition hover:border-slate-400 hover:text-slate-950 disabled:cursor-not-allowed disabled:text-slate-400"
              >
                No
              </button>
            </div>
            {turnActionError ? (
              <div className="mt-3 rounded-[18px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
                {turnActionError}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>

      <form onSubmit={handleSubmit} className="rounded-[28px] border border-slate-200 bg-white p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.24em] text-slate-500">
              Continuous Chat
            </p>
            <p className="mt-1 text-sm text-slate-600">
              {awaitingContinuation
                ? "Conferma prima se continuare o interrompere la risposta troncata."
                : canSend
                ? "Il PID e' residente in RAM/Swap e aspetta il prossimo input."
                : "Il composer si abilita quando il processo entra in WaitingForInput."}
            </p>
          </div>
          <button
            type="submit"
            disabled={!canSend || composerLoading || !composerValue.trim()}
            className="rounded-full bg-slate-950 px-5 py-2.5 text-sm font-semibold text-white transition hover:bg-slate-800 disabled:cursor-not-allowed disabled:bg-slate-300"
          >
            {composerLoading ? "Invio..." : "Send"}
          </button>
        </div>

        <textarea
          value={composerValue}
          onChange={(event) => onComposerChange(event.target.value)}
          placeholder="Invia il prossimo prompt allo stesso PID residente..."
          disabled={!canSend || composerLoading || turnActionLoading}
          className="mt-4 min-h-[112px] w-full resize-y rounded-[22px] border border-slate-200 bg-slate-50 px-4 py-3 text-sm leading-6 text-slate-800 outline-none transition placeholder:text-slate-400 focus:border-slate-400 focus:bg-white disabled:cursor-not-allowed disabled:bg-slate-100"
        />

        {composerError ? (
          <div className="mt-3 rounded-[18px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
            {composerError}
          </div>
        ) : null}
      </form>
    </section>
  );
}
