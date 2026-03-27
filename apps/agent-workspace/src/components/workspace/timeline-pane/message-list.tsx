import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Brain, Sparkles, TerminalSquare, User } from "lucide-react";

import type { TimelineItem } from "../../../lib/api";
import { ToolCallBlock } from "./tool-call-block";

interface MessageListProps {
  items: TimelineItem[];
}

export function MessageList({ items }: MessageListProps) {
  if (items.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-20 text-center opacity-60">
        <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-slate-200">
          <TerminalSquare className="h-8 w-8 text-slate-400" />
        </div>
        <p className="max-w-sm text-sm text-slate-500">
          Nessun evento disponibile per questo PID. Le nuove sessioni popolano questa Timeline
          in tempo reale.
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-4xl space-y-6">
      {items.map((item) => {
        if (item.kind === "user_message") {
          return (
            <div key={item.id} className="flex justify-end gap-4">
              <div className="max-w-[80%] rounded-2xl rounded-tr-sm bg-indigo-600 px-5 py-3.5 text-sm leading-relaxed text-white shadow-sm">
                {item.text}
              </div>
              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-indigo-200 bg-indigo-100">
                <User className="h-4 w-4 text-indigo-600" />
              </div>
            </div>
          );
        }

        if (item.kind === "assistant_message") {
          return (
            <div key={item.id} className="flex gap-4">
              <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-emerald-200 bg-emerald-100">
                <Sparkles className="h-4 w-4 text-emerald-600" />
              </div>
              <article className="max-w-[85%] rounded-2xl rounded-tl-sm border border-slate-100 bg-white px-6 py-5 text-[15px] leading-7 text-slate-700 shadow-sm">
                <div className="mb-3 flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-400">
                  AgenticOS
                  {item.status === "streaming" || item.status === "degraded-live" ? (
                    <span className="animate-pulse rounded-full bg-indigo-50 px-2 py-0.5 text-[10px] tracking-widest text-indigo-600">
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
            <div key={item.id} className="ml-12 flex gap-4">
              <details
                className="group max-w-[80%] rounded-2xl border border-slate-200 bg-slate-50/80 p-4 text-sm text-slate-700 shadow-sm"
                open={item.status === "streaming"}
              >
                <summary className="flex cursor-pointer list-none items-center gap-2 font-medium text-slate-500 transition-colors hover:text-slate-800">
                  <Brain className="h-4 w-4" />
                  Thinking Process
                  {item.status === "streaming" && (
                    <span className="ml-2 animate-pulse rounded-full bg-slate-200 px-2 py-0.5 text-[10px] uppercase tracking-wider text-slate-600">
                      Active
                    </span>
                  )}
                </summary>
                <div className="mt-4 rounded-xl border border-slate-100 bg-white p-4 font-mono text-[13px] leading-relaxed text-slate-600 whitespace-pre-wrap">
                  {item.text}
                </div>
              </details>
            </div>
          );
        }

        if (item.kind === "tool_call" || item.kind === "action_call") {
          return <ToolCallBlock key={item.id} item={item} />;
        }

        return null;
      })}
    </div>
  );
}
