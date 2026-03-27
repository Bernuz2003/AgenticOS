import { Send, Wrench } from "lucide-react";

import type { TimelineItem } from "../../../lib/api";
import { normalizeToolExecutionText } from "../../../lib/timeline/normalization";

interface ToolCallBlockProps {
  item: TimelineItem;
}

export function ToolCallBlock({ item }: ToolCallBlockProps) {
  const isAction = item.kind === "action_call";
  const liveBadge =
    item.status === "streaming"
      ? "streaming"
      : item.status === "dispatching"
        ? "dispatching"
        : null;
  const Icon = isAction ? Send : Wrench;
  const wrapperTone = isAction
    ? "border-amber-200 bg-amber-50/60 text-amber-950"
    : "border-indigo-100 bg-indigo-50/50 text-indigo-900";
  const accentTone = isAction
    ? "text-amber-700 bg-amber-200 text-amber-800 bg-amber-950 text-amber-100"
    : "text-indigo-700 bg-indigo-200 text-indigo-800 bg-indigo-950 text-indigo-100";
  const renderedText = isAction ? item.text : normalizeToolExecutionText(item.text);

  return (
    <div className="ml-12 flex gap-4">
      <div className={`w-full max-w-[80%] rounded-2xl border p-4 text-sm shadow-sm ${wrapperTone}`}>
        <div className={`flex items-center gap-2 font-semibold ${accentTone.split(" ")[0]}`}>
          <Icon className="h-4 w-4" />
          {isAction ? "Runtime Action" : "Tool Execution"}
          {liveBadge && (
            <span className={`rounded-full px-2 py-0.5 text-[10px] uppercase tracking-wider animate-pulse ${accentTone.split(" ").slice(1, 3).join(" ")}`}>
              {liveBadge}
            </span>
          )}
        </div>
        <div className={`mt-3 overflow-x-auto rounded-xl px-4 py-3 font-mono text-[12px] ${accentTone.split(" ").slice(3).join(" ")}`}>
          {renderedText}
        </div>
      </div>
    </div>
  );
}
