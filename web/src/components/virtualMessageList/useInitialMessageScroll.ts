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
};

export function shouldAcceptLateScrollRestoration(input: {
  previousRequestKey: string;
  nextRequestKey: string;
  reentryDeadline: number;
  now: number;
}): boolean {
  return (
    input.previousRequestKey === "bottom" &&
    input.nextRequestKey !== "bottom" &&
    input.now <= input.reentryDeadline
  );
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
        })
      )
        return;
    }
    didInitialScrollRef.current = true;
    requestRef.current = requestKey;
    scheduleScroll(() => {
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
    messages,
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
