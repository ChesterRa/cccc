import { useCallback, useEffect, useRef, useState } from "react";
import type { LedgerEvent } from "../../types";
import type { ChatFollowMode } from "../../stores/useUIStore";

export function useReplyTargetNavigation(input: {
  messages: LedgerEvent[];
  shouldVirtualize: boolean;
  missingTargetMessage: string;
  parentRef: React.MutableRefObject<HTMLDivElement | null>;
  onScrollTopChange: (top: number) => void;
  getMessageRowById: (eventId: string) => HTMLDivElement | null;
  scrollToIndexStable: (index: number) => void;
  cancelPendingBottomScroll: () => void;
  setAtBottom: (value: boolean) => void;
  setFollowMode: (value: ChatFollowMode) => void;
}) {
  const clearHighlightTimerRef = useRef<number | null>(null);
  const clearNoticeTimerRef = useRef<number | null>(null);
  const [highlightEventId, setHighlightEventId] = useState("");
  const [notice, setNotice] = useState("");

  const showNotice = useCallback((message: string) => {
    if (clearNoticeTimerRef.current != null) window.clearTimeout(clearNoticeTimerRef.current);
    setNotice(message);
    clearNoticeTimerRef.current = window.setTimeout(() => {
      clearNoticeTimerRef.current = null;
      setNotice("");
    }, 2200);
  }, []);

  const openReplyTarget = useCallback(
    (replyToEventId: string) => {
      const targetId = String(replyToEventId || "").trim();
      if (!targetId) return;
      const index = input.messages.findIndex((message) => String(message?.id || "") === targetId);
      if (index < 0) {
        showNotice(input.missingTargetMessage);
        return;
      }

      input.setAtBottom(false);
      input.setFollowMode("detached");
      input.cancelPendingBottomScroll();
      setHighlightEventId(targetId);
      if (clearHighlightTimerRef.current != null) {
        window.clearTimeout(clearHighlightTimerRef.current);
      }
      clearHighlightTimerRef.current = window.setTimeout(() => {
        clearHighlightTimerRef.current = null;
        setHighlightEventId((current) => (current === targetId ? "" : current));
      }, 2200);

      if (input.shouldVirtualize) {
        input.scrollToIndexStable(index);
        return;
      }
      const container = input.parentRef.current;
      const row = input.getMessageRowById(targetId);
      if (!container || !row) {
        showNotice(input.missingTargetMessage);
        return;
      }
      const top = Math.max(
        0,
        row.offsetTop - Math.max(0, (container.clientHeight - row.offsetHeight) / 2),
      );
      container.scrollTo({ top, behavior: "auto" });
      input.onScrollTopChange(top);
    },
    [input, showNotice],
  );

  useEffect(
    () => () => {
      if (clearHighlightTimerRef.current != null) {
        window.clearTimeout(clearHighlightTimerRef.current);
      }
      if (clearNoticeTimerRef.current != null) window.clearTimeout(clearNoticeTimerRef.current);
    },
    [],
  );

  return { replyJumpHighlightId: highlightEventId, replyJumpNotice: notice, openReplyTarget };
}
