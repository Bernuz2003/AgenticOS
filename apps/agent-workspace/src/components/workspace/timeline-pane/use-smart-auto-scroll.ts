import { useCallback, useEffect, useRef } from "react";

const BOTTOM_THRESHOLD_PX = 48;

function isPinnedToBottom(container: HTMLDivElement): boolean {
  const distanceFromBottom =
    container.scrollHeight - container.scrollTop - container.clientHeight;
  return distanceFromBottom <= BOTTOM_THRESHOLD_PX;
}

export function useSmartAutoScroll(contentKey: string, resetKey: string) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = "auto") => {
    const container = scrollRef.current;
    if (!container) {
      return;
    }

    container.scrollTo({
      top: container.scrollHeight,
      behavior,
    });
  }, []);

  const handleScroll = useCallback(() => {
    const container = scrollRef.current;
    if (!container) {
      return;
    }

    stickToBottomRef.current = isPinnedToBottom(container);
  }, []);

  useEffect(() => {
    stickToBottomRef.current = true;
    const frame = window.requestAnimationFrame(() => scrollToBottom("auto"));
    return () => window.cancelAnimationFrame(frame);
  }, [resetKey, scrollToBottom]);

  useEffect(() => {
    if (!stickToBottomRef.current) {
      return;
    }

    const frame = window.requestAnimationFrame(() => scrollToBottom("auto"));
    return () => window.cancelAnimationFrame(frame);
  }, [contentKey, scrollToBottom]);

  return {
    scrollRef,
    handleScroll,
  };
}
