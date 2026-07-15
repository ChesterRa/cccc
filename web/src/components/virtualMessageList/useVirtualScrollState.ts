import { useCallback, useRef } from "react";
import type { ChatFollowMode } from "../../stores/useUIStore";
import type { LedgerEvent } from "../../types";
import { getChatTailMutationSnapshot, getChatTailSnapshot } from "../../utils/chatAutoFollow";
import { estimateMessageRowHeight } from "../messageBubble/estimate";
import { getStableMessageKey } from "../virtualMessageListHelpers";
import { getMessageRowGrouping } from "./grouping";

export function useVirtualScrollState(messages: LedgerEvent[]) {
  const tailKey =
    messages.length > 0
      ? getStableMessageKey(messages[messages.length - 1], messages.length - 1)
      : null;

  const isAtBottomRef = useRef(true);
  const followModeRef = useRef<ChatFollowMode>("follow");
  const prevTailSnapshotRef = useRef(getChatTailSnapshot(tailKey, messages.length));
  const prevTailMutationSnapshotRef = useRef(getChatTailMutationSnapshot(tailKey, ""));
  const didInitialScrollRef = useRef(false);
  const initialScrollRequestRef = useRef("");
  const initialScrollReentryDeadlineRef = useRef(0);
  const scrollRafRef = useRef<number | null>(null);
  const scrollTokenRef = useRef(0);
  const bottomScrollRequestTokenRef = useRef(0);
  const scrollRafScheduledRef = useRef(false);
  const snapshotFlushTimerRef = useRef<number | null>(null);
  const lastScrollTopRef = useRef(0);
  const previousContentSizeRef = useRef(0);
  const isContainerResizingRef = useRef(false);
  const forceStickToBottomUntilRef = useRef(0);
  const prevResetKeyRef = useRef<string>();
  const latestSnapshotRef = useRef<{
    mode: ChatFollowMode;
    anchorId: string;
    offsetPx: number;
    updatedAt: number;
  } | null>(null);
  const getEstimatedSize = useCallback((index: number) => {
    const message = messages[index];
    const previous = index > 0 ? messages[index - 1] : undefined;
    return estimateMessageRowHeight(message, {
      collapseHeader: getMessageRowGrouping(previous, message).collapseHeader,
    });
  }, [messages]);

  return {
    isAtBottomRef,
    followModeRef,
    prevTailSnapshotRef,
    prevTailMutationSnapshotRef,
    didInitialScrollRef,
    initialScrollRequestRef,
    initialScrollReentryDeadlineRef,
    scrollRafRef,
    scrollTokenRef,
    bottomScrollRequestTokenRef,
    scrollRafScheduledRef,
    snapshotFlushTimerRef,
    lastScrollTopRef,
    previousContentSizeRef,
    isContainerResizingRef,
    forceStickToBottomUntilRef,
    prevResetKeyRef,
    latestSnapshotRef,
    getEstimatedSize,
  };
}
