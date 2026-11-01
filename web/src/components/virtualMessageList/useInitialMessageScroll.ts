import { useEffect } from "react";
import type { MutableRefObject } from "react";
import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  type ChatFollowMode,
  type ChatScrollSnapshot,
} from "../../stores/useUIStore";
import type { LedgerEvent } from "../../types";
import { getScrollRestorationRequestKey } from "./useScrollAnchorRestoration";

type InitialMessageScrollOptions = {
  messages: LedgerEvent[];
  didInitialScrollRef: MutableRefObject<boolean>;
  requestRef: MutableRefObject<string>;
  reentryDeadlineRef: MutableRefObject<number>;
  targetId?: string;
  anchorId?: string;
  anchorOffsetPx?: number;
  shouldVirtualize: boolean;
  scheduleScroll: (action: () => void) => void;
  scrollToIndex: (index: number) => void;
  scrollToMessageAnchor: (eventId: string, offsetPx?: number) => boolean;
  beginAnchorRestoration: (anchor: { anchorId: string; offsetPx: number }) => boolean;
  setAtBottom: (value: boolean) => void;
  setFollowMode: (mode: ChatFollowMode) => void;
  scheduleForceStickToBottom: () => void;
  onScrollSnapshot?: (snapshot: ChatScrollSnapshot) => void;
  onRestoreAwayFromBottom?: () => void;
  /** Whether the reader has acted in this view since it opened. */
  isEngaged?: () => boolean;
  /** Whether the anchor stands in for a message the reader did not see here. */
  highlightAnchor?: boolean;
  /** Told the anchor once the view opens on it, when it is to be marked. */
  onHighlightAnchor?: (anchorId: string) => void;
};

// A place that arrives just after the view opened, such as the departing filter's own, still
// applies until the reader acts here; once they have, nothing late moves the view. Without that
// signal only a view that opened at the bottom takes one. It never sends a view to the bottom.
export function shouldAcceptLateScrollRestoration(input: {
  previousRequestKey: string;
  nextRequestKey: string;
  reentryDeadline: number;
  now: number;
  engaged?: boolean;
}): boolean {
  const stillOpening =
    input.engaged === undefined ? input.previousRequestKey === "bottom" : !input.engaged;
  return stillOpening && input.nextRequestKey !== "bottom" && input.now <= input.reentryDeadline;
}

export function useInitialMessageScroll({
  messages,
  didInitialScrollRef,
  requestRef,
  reentryDeadlineRef,
  targetId,
  anchorId,
  anchorOffsetPx,
  shouldVirtualize,
  scheduleScroll,
  scrollToIndex,
  scrollToMessageAnchor,
  beginAnchorRestoration,
  setAtBottom,
  setFollowMode,
  scheduleForceStickToBottom,
  onScrollSnapshot,
  onRestoreAwayFromBottom,
  isEngaged,
  highlightAnchor = false,
  onHighlightAnchor,
}: InitialMessageScrollOptions) {
  useEffect(() => {
    if (messages.length <= 0) return;
    const requestKey = getScrollRestorationRequestKey({
      targetId,
      anchorId,
      offsetPx: anchorOffsetPx,
    });
    if (didInitialScrollRef.current) {
      if (requestRef.current === requestKey) return;
      if (
        !shouldAcceptLateScrollRestoration({
          previousRequestKey: requestRef.current,
          nextRequestKey: requestKey,
          reentryDeadline: reentryDeadlineRef.current,
          now: Date.now(),
          engaged: isEngaged?.(),
        })
      )
        return;
    }
    didInitialScrollRef.current = true;
    requestRef.current = requestKey;
    scheduleScroll(() => {
      // A reader who acts before the view settles keeps the place they chose.
      if (isEngaged?.()) return;
      const markRestoredAwayFromBottom = () => {
        setAtBottom(false);
        setFollowMode("detached");
        onRestoreAwayFromBottom?.();
      };
      if (targetId) {
        if (shouldVirtualize) {
          const index = messages.findIndex(
            (message) => String(message?.id || "") === String(targetId),
          );
          if (index >= 0) {
            markRestoredAwayFromBottom();
            scrollToIndex(index);
            return;
          }
        } else if (scrollToMessageAnchor(String(targetId), 0)) {
          markRestoredAwayFromBottom();
          return;
        }
      }
      if (anchorId) {
        if (
          beginAnchorRestoration({
            anchorId: String(anchorId),
            offsetPx: Number(anchorOffsetPx || 0),
          })
        ) {
          markRestoredAwayFromBottom();
          if (highlightAnchor) onHighlightAnchor?.(String(anchorId));
          return;
        }
        onScrollSnapshot?.({
          coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
          mode: "follow",
          anchorId: "",
          offsetPx: 0,
          updatedAt: Date.now(),
        });
      }
      setAtBottom(true);
      setFollowMode("follow");
      scheduleForceStickToBottom();
    });
  }, [
    anchorId,
    anchorOffsetPx,
    beginAnchorRestoration,
    didInitialScrollRef,
    highlightAnchor,
    isEngaged,
    messages,
    onHighlightAnchor,
    onRestoreAwayFromBottom,
    onScrollSnapshot,
    reentryDeadlineRef,
    requestRef,
    scheduleForceStickToBottom,
    scheduleScroll,
    scrollToIndex,
    scrollToMessageAnchor,
    setAtBottom,
    setFollowMode,
    shouldVirtualize,
    targetId,
  ]);
}
