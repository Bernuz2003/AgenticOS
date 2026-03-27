import type { TimelineItem } from "../api";

export function hiddenTimelineItemCount(
  allItems: TimelineItem[] | undefined,
  visibleCount: number,
): number {
  return Math.max(0, (allItems?.length ?? 0) - visibleCount);
}
