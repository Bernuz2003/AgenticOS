import type { TimelineItem } from "../api";

export function filterRenderableTimelineItems(items: TimelineItem[]): TimelineItem[] {
  return items.filter(
    (item) => item.kind !== "tool_result" && item.kind !== "system_event",
  );
}

export function buildTimelineSignature(items: TimelineItem[]): string {
  return items.map((item) => `${item.id}:${item.text.length}:${item.status}`).join("|");
}
