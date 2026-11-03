import { memo, useRef, useEffect, useLayoutEffect, useCallback, useMemo, useState } from "react";
import { measureElement as measureVirtualElement, useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import { ArrowDownIcon } from "./Icons";
import { LcdSpinner } from "./LcdSpinner";
import { getChatTailMutationSnapshot, getChatTailSnapshot } from "../utils/chatAutoFollow";
import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  type ChatFollowMode,
  type ChatScrollSnapshot,
} from "../stores/useUIStore";
import {
  getStableMessageKey,
  isVirtualizedScrollNearEnd,
  shouldAutoScrollToBottom,
  shouldDetachChatFollowOnScroll,
  shouldNotifyScrollChange,
  shouldPromoteScrollToFollow,
  shouldUseVirtualizedMessageList,
  VIRTUAL_OVERSCAN_ROWS,
  wasAtBottomBeforeContentChange,
} from "./virtualMessageListHelpers";
import {
  getCorrectedScrollTopForAnchor,
  usePrependCompensationController,
  useTopHistoryLoadCoordinator,
} from "./virtualMessageListPrependCompensation";
import { classNames } from "../utils/classNames";
import { subscribeUiStylePrefs } from "../utils/uiStyle";
import { getNonVirtualMessageListTopMargin } from "./virtualMessageListLayout";
import { useVirtualMessageMetadata } from "./virtualMessageList/useVirtualMessageMetadata";
import type { VirtualMessageListProps } from "./virtualMessageList/types";
import { useReplyTargetNavigation } from "./virtualMessageList/useReplyTargetNavigation";
import { MessageRows } from "./virtualMessageList/MessageRows";
import { useVirtualScrollState } from "./virtualMessageList/useVirtualScrollState";
import { useScrollAnchorRestoration as useAnchorRestoration } from "./virtualMessageList/useScrollAnchorRestoration";
import { useInitialMessageScroll } from "./virtualMessageList/useInitialMessageScroll";
import { getReadingFocus, useReadingEngagement } from "./virtualMessageList/useReadingEngagement";
import { useMarginWheel } from "./virtualMessageList/useMarginWheel";
import { useMessageTailAutoFollow } from "./virtualMessageList/useMessageTailAutoFollow";
import { usePendingReplyFollow } from "./virtualMessageList/usePendingReplyFollow";
import { cacheMessageRowHeight } from "./virtualMessageList/rowHeightCache";
import {
  discardDeferredScrollAdjustment,
  getFreshVirtualOffsetForIndex,
  shouldAdjustScrollForResizedRow,
} from "./virtualMessageList/virtualOffset";
import {
  getMessageAnchorOffset,
  getReadingTopAnchorOffset,
  getScrollOffsetForMessageAnchor,
} from "./virtualMessageListAnchorRestore";
import { useSendScrollRequestLifecycle } from "./virtualMessageList/useSendScrollRequestLifecycle";
import { useBottomScrollController } from "./virtualMessageList/useBottomScrollController";
import {
  useForcedBottomFollow,
  useForcedBottomFollowKeyboardCancel as useBottomFollowKeyCancel,
} from "./virtualMessageList/useForcedBottomFollow";
import { useViewChangeAutoFollow } from "./virtualMessageList/useViewChangeAutoFollow";
import { VirtualMessageListEmptyState } from "./virtualMessageList/VirtualMessageListEmptyState";

export type { VirtualMessageListProps } from "./virtualMessageList/types";

type VirtualMessageListInnerProps = VirtualMessageListProps & { resetKey: string };

const HISTORY_TOP_TRIGGER_PX = 80;
const HISTORY_TOP_REARM_PX = 240;

const VirtualMessageListInner = function VirtualMessageListInner({
  messages,
  actors,
  agentStates,
  taskById,
  isDark,
  readOnly,
  groupId,
  groupLabelById,
  webModelDeliveryStatusByEventId,
  viewKey,
  followOnViewChangeKey,
  initialScrollTargetId,
  initialScrollAnchorId,
  initialScrollAnchorOffsetPx,
  initialScrollOffsetPx,
  highlightInitialScrollAnchor = false,
  highlightEventId,
  liveJumpActive,
  contextWindow,
  className,
  topInsetPx = 0,
  scrollRef,
  onReply,
  onShowRecipients,
  onCopyLink,
  onTogglePin,
  onViewContext,
  onRelay,
  onOpenSource,
  onOpenPresentationRef,
  onOpenTaskRef,
  showScrollButton,
  onScrollButtonClick,
  chatUnreadCount,
  onScrollChange,
  onContextReady,
  onReachedContextEnd,
  onLoadNewerContext,
  isLoadingNewerContext,
  newerContextError,
  onScrollSnapshot,
  sendScrollRequest,
  onSendScrollRequestConsumed,
  isLoadingHistory = false,
  hasMoreHistory = true,
  hasActiveFilters = false,
  isFilteredEmpty = false,
  emptyMessage,
  onLoadMore,
  resetKey,
}: VirtualMessageListInnerProps) {
  const { t } = useTranslation("chat");
  const parentRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const onScrollSnapshotRef = useRef(onScrollSnapshot);
  const captureScrollSnapshotRef = useRef<() => ChatScrollSnapshot | null>(() => null);
  const remeasureRafRef = useRef<number | null>(null);
  const contentWidthRef = useRef<number | null>(null);
  // Message ordering is resolved upstream in useChatTab. The virtual list
  // should render that order verbatim instead of maintaining a second,
  // divergent streaming-order cache locally.
  const displayMessages = messages;
  const { messageTextById, agentStateById, actorById, displayNameMap } = useVirtualMessageMetadata(
    displayMessages,
    actors,
    agentStates,
  );
  const shouldVirtualize = shouldUseVirtualizedMessageList(displayMessages.length);
  const topInset = Math.max(0, Number(topInsetPx) || 0);
  const ownerViewKey = String(viewKey ?? groupId);

  const {
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
    forceStickToBottomRef,
    prevResetKeyRef,
    latestSnapshotRef,
    getEstimatedSize,
  } = useVirtualScrollState(displayMessages, resetKey);

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: displayMessages.length,
    enabled: shouldVirtualize,
    getScrollElement: () => parentRef.current,
    getItemKey: (index) => getStableMessageKey(displayMessages[index], index),
    estimateSize: getEstimatedSize,
    measureElement: (element, entry, instance) => {
      const height = measureVirtualElement(element, entry, instance);
      const index = Number(element.getAttribute("data-index"));
      if (Number.isInteger(index) && index >= 0 && index < displayMessages.length) {
        // Switching to Terminals hides these rows. A zero-size notification is
        // not a new message height and must not collapse the virtual history.
        if (height === 0) return getEstimatedSize(index);
        cacheMessageRowHeight(resetKey, getStableMessageKey(displayMessages[index], index), height);
      }
      return height;
    },
    initialOffset: Math.max(0, Number(initialScrollOffsetPx) || 0),
    overscan: VIRTUAL_OVERSCAN_ROWS,
    paddingStart: 72 + topInset,
  });

  // The default rule misjudges rows measured in one batch after a scroll jump
  // (see shouldAdjustScrollForResizedRow).
  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = shouldAdjustScrollForResizedRow;

  // Let tanstack own row measurement via its built-in observer. Layering an
  // extra per-row ResizeObserver on top of measureElement creates duplicate
  // measure -> notify cycles that can recurse during rapid scrolling.
  const measureElement = virtualizer.measureElement;

  const getMessageRowById = useCallback((eventId: string): HTMLDivElement | null => {
    const container = parentRef.current;
    if (!container || !eventId) return null;
    return container.querySelector(
      `[data-message-row="true"][data-message-id="${CSS.escape(eventId)}"]`,
    );
  }, []);

  // A view the reader only looks at writes no snapshot, so returning keeps the saved place;
  // once they act here, snapshots carry this view and the message being read.
  const {
    engagedRef,
    touchedIdRef,
    scrollIntentAtRef,
    scrollIntentStartTopRef,
    scrollIntentIdRef,
  } = useReadingEngagement(parentRef);
  useMarginWheel(parentRef);
  const consumedContextScrollIntentRef = useRef(0);
  const awaitingLiveEndRef = useRef(false);
  const onReachedContextEndRef = useRef(onReachedContextEnd);
  const onContextReadyRef = useRef(onContextReady);
  onReachedContextEndRef.current = onReachedContextEnd;
  onContextReadyRef.current = onContextReady;
  const optimisticIds = useMemo(
    () =>
      new Set(
        displayMessages
          .filter((message) => (message.data as { _optimistic?: unknown } | undefined)?._optimistic)
          .map((message) => String(message.id || "")),
      ),
    [displayMessages],
  );
  const withReadingFocus = useCallback(
    (snapshot: ChatScrollSnapshot): ChatScrollSnapshot => {
      const el = parentRef.current;
      const reading = el
        ? getReadingFocus(el, topInset, touchedIdRef.current, (id) => !optimisticIds.has(id))
        : null;
      const owned = { ...snapshot, viewKey: ownerViewKey };
      if (!reading) return owned;
      const { focusId, focusOffsetPx, visible, touched } = reading;
      if (snapshot.mode === "detached") return { ...owned, focusId, focusOffsetPx, visible };
      // At the bottom only a message the reader touched travels: a list too short to scroll is
      // always there, and without a touch the next view opens at its own bottom as well.
      return touched ? { ...owned, focusId, focusOffsetPx, visible: [visible[0]] } : owned;
    },
    [optimisticIds, ownerViewKey, topInset, touchedIdRef],
  );
  // A list too short to scroll is at its own bottom, yet no scroll tells the page so, and the
  // page should not be told: it would stop counting new messages this view filters out. Its key
  // to the latest then has nothing to do until such a message is counted. Any scroll proves the
  // list scrolls; a virtual list is long, though it may lack its height while it measures.
  const [unscrollable, setUnscrollable] = useState(false);
  useLayoutEffect(() => {
    const el = parentRef.current;
    setUnscrollable(!!el && !shouldVirtualize && el.scrollHeight - el.clientHeight < 8);
  }, [displayMessages, shouldVirtualize]);

  // Short lists cannot scroll far enough to rearm the top-load gate after a page
  // with no visible results. Observe both the viewport and the message content:
  // resizing the window or expanding an attachment can change that distance.
  const [shortForHistory, setShortForHistory] = useState(false);
  useLayoutEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const measure = () => {
      setShortForHistory(
        el.clientHeight > 0 && el.scrollHeight - el.clientHeight <= HISTORY_TOP_REARM_PX,
      );
    };
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    if (contentRef.current) observer.observe(contentRef.current);
    return () => observer.disconnect();
  }, [displayMessages.length, shouldVirtualize, topInset]);
  // A stand-in for a message the reader did not see here is marked once the view opens on it,
  // as long as a reply jump's mark.
  const [arrivalHighlightId, setArrivalHighlightId] = useState("");
  const arrivalHighlightTimerRef = useRef<number | null>(null);
  const markArrival = useCallback((anchorId: string) => {
    setArrivalHighlightId(anchorId);
    if (arrivalHighlightTimerRef.current != null) {
      window.clearTimeout(arrivalHighlightTimerRef.current);
    }
    arrivalHighlightTimerRef.current = window.setTimeout(() => {
      arrivalHighlightTimerRef.current = null;
      setArrivalHighlightId("");
    }, 2200);
  }, []);
  useEffect(
    () => () => {
      if (arrivalHighlightTimerRef.current != null) {
        window.clearTimeout(arrivalHighlightTimerRef.current);
      }
    },
    [],
  );
  const writeEngagedSnapshot = useCallback(
    (snapshot: ChatScrollSnapshot) => {
      if (engagedRef.current) onScrollSnapshot?.(snapshot);
    },
    [engagedRef, onScrollSnapshot],
  );
  const isEngaged = useCallback(() => engagedRef.current, [engagedRef]);
  const markEngaged = useCallback(() => {
    engagedRef.current = true;
  }, [engagedRef]);

  const getAnchorSnapshot = useCallback(
    (scrollTop: number) => {
      const container = parentRef.current;
      if (!container) return null;

      const containerRect = container.getBoundingClientRect();
      const renderedRows = Array.from(
        container.querySelectorAll<HTMLDivElement>('[data-message-row="true"]'),
      );
      const visibleRow = renderedRows.find((row) => {
        const rect = row.getBoundingClientRect();
        return rect.bottom > containerRect.top + 1 && rect.top < containerRect.bottom - 1;
      });
      if (visibleRow) {
        const anchorId = String(visibleRow.dataset.messageId || "").trim();
        if (anchorId) {
          return { anchorId, offsetPx: containerRect.top - visibleRow.getBoundingClientRect().top };
        }
      }

      if (shouldVirtualize) {
        const vItems = virtualizer.getVirtualItems();
        if (vItems.length <= 0) return null;
        const anchorItem = vItems.find((v) => v.start + v.size > scrollTop + 1) || vItems[0];
        const msg = displayMessages[anchorItem.index];
        const anchorId = msg?.id ? String(msg.id) : "";
        if (!anchorId) return null;
        return { anchorId, offsetPx: getMessageAnchorOffset(scrollTop, anchorItem.start) };
      }

      if (renderedRows.length <= 0) return null;
      const anchorRow = renderedRows[0];
      const anchorId = String(anchorRow.dataset.messageId || "").trim();
      if (!anchorId) return null;
      return { anchorId, offsetPx: getMessageAnchorOffset(scrollTop, anchorRow.offsetTop) };
    },
    [displayMessages, shouldVirtualize, virtualizer],
  );

  const getCurrentContentSize = useCallback(() => {
    const el = parentRef.current;
    if (!el) return 0;
    return shouldVirtualize ? virtualizer.getTotalSize() : el.scrollHeight;
  }, [shouldVirtualize, virtualizer]);

  const setAtBottom = useCallback(
    (next: boolean) => {
      isAtBottomRef.current = next;
    },
    [isAtBottomRef],
  );

  const notifyRestoredAwayFromBottom = useCallback(() => {
    onScrollChange?.(false);
  }, [onScrollChange]);

  const markAwayFromBottomForHistoryLoad = useCallback(() => {
    setAtBottom(false);
  }, [setAtBottom]);

  const scrollToMessageAnchor = useCallback(
    (eventId: string, offsetPx = 0) => {
      const el = parentRef.current;
      if (!el || !eventId) return false;

      if (shouldVirtualize) {
        const idx = displayMessages.findIndex((m) => String(m?.id || "") === String(eventId));
        if (idx < 0) return false;
        const rowOffset = getFreshVirtualOffsetForIndex(virtualizer, idx, contentRef.current);
        if (rowOffset != null) {
          virtualizer.scrollToOffset(getScrollOffsetForMessageAnchor(rowOffset, offsetPx), {
            align: "start",
            behavior: "auto",
          });
        } else {
          virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
        }
        return true;
      }

      const row = getMessageRowById(String(eventId));
      if (!row) return false;
      el.scrollTo({
        top: getScrollOffsetForMessageAnchor(row.offsetTop, offsetPx),
        behavior: "auto",
      });
      return true;
    },
    [displayMessages, getMessageRowById, shouldVirtualize, virtualizer],
  );

  const scrollToVirtualOffset = useCallback(
    (offsetPx: number) => {
      virtualizer.scrollToOffset(offsetPx, { align: "start", behavior: "auto" });
    },
    [virtualizer],
  );

  // scrollToIndex re-targets the row every frame as rows get measured, so it
  // reaches a row whose estimated offset was wrong.
  const revealMessageAnchor = useCallback(
    (eventId: string) => {
      if (!shouldVirtualize) return;
      const idx = displayMessages.findIndex((m) => String(m?.id || "") === String(eventId));
      if (idx >= 0) virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
    },
    [displayMessages, shouldVirtualize, virtualizer],
  );

  const discardDeferredVirtualScroll = useCallback(() => {
    if (shouldVirtualize) discardDeferredScrollAdjustment(virtualizer);
  }, [shouldVirtualize, virtualizer]);

  const prependCompensation = usePrependCompensationController({
    parentRef,
    lastScrollTopRef,
    getMessageRowById,
    isVirtualized: shouldVirtualize,
    scrollToVirtualOffset,
    onAnchorVerified: discardDeferredVirtualScroll,
  });
  const getAnchorTop = useCallback(
    (anchorId: string) => {
      const row = getMessageRowById(anchorId);
      return row ? row.getBoundingClientRect().top : null;
    },
    [getMessageRowById],
  );

  const checkIsAtBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el) return true;
    const threshold = 8;
    const scrollAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
    if (!scrollAtBottom) return false;

    if (shouldVirtualize) {
      return isVirtualizedScrollNearEnd({
        virtualItems: virtualizer.getVirtualItems(),
        displayMessagesCount: displayMessages.length,
      });
    }
    return true;
  }, [displayMessages.length, shouldVirtualize, virtualizer]);

  const notifyContextEnd = useCallback(
    (initial: boolean) => {
      const el = parentRef.current;
      const tailId = String(displayMessages.at(-1)?.id || "");
      if (!onReachedContextEndRef.current || !el || el.clientHeight <= 0) return false;
      const atBottom = checkIsAtBottom();
      const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < el.clientHeight;
      if (!atBottom && (!nearBottom || initial)) return false;
      const visible = (id: string) => {
        const row = getMessageRowById(id);
        if (!row) return false;
        const box = el.getBoundingClientRect();
        const item = row.getBoundingClientRect();
        return item.bottom > box.top && item.top < box.bottom;
      };
      if (!tailId || !(atBottom ? visible(tailId) : getMessageRowById(tailId))) return false;
      if (
        initial &&
        (engagedRef.current || !initialScrollTargetId || !visible(initialScrollTargetId))
      )
        return false;
      const result = onReachedContextEndRef.current(initial, atBottom) === true;
      return result;
    },
    [checkIsAtBottom, displayMessages, engagedRef, getMessageRowById, initialScrollTargetId],
  );

  const captureCurrentScrollSnapshot = useCallback((): ChatScrollSnapshot | null => {
    const el = parentRef.current;
    const latest = latestSnapshotRef.current;
    // Every saved place names its view, including the fallback to the last scroll's place.
    if (!el || displayMessages.length <= 0) return latest && withReadingFocus(latest);
    if (checkIsAtBottom()) {
      return withReadingFocus({
        coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
        mode: "follow",
        anchorId: "",
        offsetPx: 0,
        scrollTop: el.scrollTop,
        updatedAt: Date.now(),
      });
    }
    const anchor = getAnchorSnapshot(el.scrollTop);
    if (!anchor) return latest && withReadingFocus(latest);
    return withReadingFocus({
      coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
      mode: "detached",
      anchorId: anchor.anchorId,
      offsetPx: anchor.offsetPx,
      scrollTop: el.scrollTop,
      updatedAt: Date.now(),
    });
  }, [
    checkIsAtBottom,
    displayMessages.length,
    getAnchorSnapshot,
    latestSnapshotRef,
    withReadingFocus,
  ]);

  useLayoutEffect(() => {
    captureScrollSnapshotRef.current = captureCurrentScrollSnapshot;
  }, [captureCurrentScrollSnapshot]);

  const { activeRequestRef: activeSendScrollRequestRef, scrollToBottom } =
    useBottomScrollController({
      parentRef,
      messageCount: displayMessages.length,
      groupId,
      viewKey: ownerViewKey,
      requestTokenRef: bottomScrollRequestTokenRef,
      followModeRef,
      isAtBottomRef,
      forceStickToBottomRef,
    });

  const {
    cancelPendingBottomScroll,
    cancelScheduledScroll,
    scheduleForceStickToBottom,
    shouldForceStickToBottom,
  } = useForcedBottomFollow({
    requestTokenRef: bottomScrollRequestTokenRef,
    scrollRafRef,
    forceStickToBottomRef,
    scrollToBottom,
  });

  const applyRestoredAnchor = useCallback(
    ({ anchorId, offsetPx }: { anchorId: string; offsetPx: number }) => {
      const el = parentRef.current;
      if (!el) return false;

      const row = getMessageRowById(anchorId);
      if (row) {
        const desiredAnchorTop = el.getBoundingClientRect().top - offsetPx;
        const correctedTop = getCorrectedScrollTopForAnchor({
          currentScrollTop: el.scrollTop,
          lockedAnchorTop: desiredAnchorTop,
          currentAnchorTop: row.getBoundingClientRect().top,
          minDeltaPx: 0.5,
        });
        if (correctedTop !== el.scrollTop) {
          el.scrollTop = Math.max(0, correctedTop);
        }
        lastScrollTopRef.current = el.scrollTop;
        return true;
      }

      if (shouldVirtualize) {
        const idx = displayMessages.findIndex(
          (message) => String(message?.id || "") === String(anchorId),
        );
        if (idx < 0) return false;
        const rowOffset = getFreshVirtualOffsetForIndex(virtualizer, idx, contentRef.current);
        if (rowOffset == null) {
          virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
          return true;
        }
        virtualizer.scrollToOffset(getScrollOffsetForMessageAnchor(rowOffset, offsetPx), {
          align: "start",
          behavior: "auto",
        });
        return true;
      }
      return false;
    },
    [displayMessages, getMessageRowById, lastScrollTopRef, shouldVirtualize, virtualizer],
  );
  const anchorRestoration = useAnchorRestoration(applyRestoredAnchor, cancelPendingBottomScroll);

  const setFollowMode = useCallback(
    (next: ChatFollowMode) => {
      if (next === "follow") anchorRestoration.cancel();
      followModeRef.current = next;
    },
    [anchorRestoration, followModeRef],
  );

  const detachForPendingReply = useCallback(() => {
    setFollowMode("detached");
    setAtBottom(false);
    onScrollChange?.(false);
  }, [onScrollChange, setAtBottom, setFollowMode]);
  const {
    cancel: cancelPendingReplyFollow,
    consider: considerPendingReply,
    revealIfNeeded: revealPendingReply,
  } = usePendingReplyFollow({
    parentRef,
    readOnly,
    topInset,
    cancelBottomScroll: cancelPendingBottomScroll,
    detach: detachForPendingReply,
  });
  const followLatestMessage = useCallback(() => {
    if (!revealPendingReply()) scrollToBottom();
  }, [revealPendingReply, scrollToBottom]);

  const scheduleInitialFollow = useCallback(() => {
    const latest = displayMessages[displayMessages.length - 1];
    if (latest) considerPendingReply(latest);
    scheduleForceStickToBottom(revealPendingReply);
  }, [displayMessages, considerPendingReply, revealPendingReply, scheduleForceStickToBottom]);

  const detachFollowModeForHistoryLoad = useCallback(() => {
    setFollowMode("detached");
  }, [setFollowMode]);

  const topHistoryLoad = useTopHistoryLoadCoordinator({
    compensation: prependCompensation,
    getAnchorSnapshot,
    getAnchorTop,
    getCurrentContentSize,
    scrollToMessageAnchor,
    revealMessageAnchor,
    cancelPendingBottomScroll,
    detachFollowMode: detachFollowModeForHistoryLoad,
    markAwayFromBottom: markAwayFromBottomForHistoryLoad,
    onLoadMore,
  });

  const captureContextAppend = useCallback(
    (scrollTop: number) => {
      const anchor = getAnchorSnapshot(scrollTop);
      return {
        previousOffset: scrollTop,
        previousTotalSize: getCurrentContentSize(),
        anchorId: anchor?.anchorId || "",
        anchorOffsetPx: anchor?.offsetPx || 0,
        anchorTop: anchor?.anchorId ? getAnchorTop(anchor.anchorId) : null,
        kind: "append" as const,
      };
    },
    [getAnchorSnapshot, getAnchorTop, getCurrentContentSize],
  );

  // The ordinary list detaches its DOM ref while it changes to virtual rows. Its most recent
  // user-scroll snapshot is still available and identifies the reader's message and offset.
  const thresholdSnapshotRef = useRef<ChatScrollSnapshot | null>(null);
  const captureThresholdRef = useRef(() => {});
  const restoreThresholdRef = useRef(() => {});
  captureThresholdRef.current = () => {
    const snapshot = latestSnapshotRef.current;
    thresholdSnapshotRef.current =
      followModeRef.current === "detached" &&
      snapshot?.mode === "detached" &&
      snapshot.anchorId &&
      !prependCompensation.pendingRef.current
        ? snapshot
        : null;
  };
  restoreThresholdRef.current = () => {
    const snapshot = thresholdSnapshotRef.current;
    thresholdSnapshotRef.current = null;
    if (!snapshot || prependCompensation.pendingRef.current) return;
    anchorRestoration.begin({ anchorId: snapshot.anchorId, offsetPx: snapshot.offsetPx });
  };
  useLayoutEffect(() => {
    if (shouldVirtualize) restoreThresholdRef.current();
    return () => {
      if (!shouldVirtualize) captureThresholdRef.current();
    };
  }, [shouldVirtualize]);

  // At the end of a scroll container, another downward gesture produces no scroll event.
  // Treat that gesture as a request to continue through the context or return to live chat.
  const continueContextAtBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el || !checkIsAtBottom() || !onReachedContextEndRef.current) return;
    if (contextWindow && !onLoadNewerContext) awaitingLiveEndRef.current = true;
    const top = el.scrollTop;
    const anchor = captureContextAppend(top);
    if (!notifyContextEnd(false)) return;
    consumedContextScrollIntentRef.current = scrollIntentIdRef.current;
    prependCompensation.begin(anchor);
    lastScrollTopRef.current = top;
    setAtBottom(false);
    setFollowMode("detached");
    cancelPendingBottomScroll();
    onScrollChange?.(false);
  }, [
    cancelPendingBottomScroll,
    captureContextAppend,
    checkIsAtBottom,
    contextWindow,
    lastScrollTopRef,
    notifyContextEnd,
    onLoadNewerContext,
    onScrollChange,
    prependCompensation,
    scrollIntentIdRef,
    setAtBottom,
    setFollowMode,
  ]);
  const contextTouchStartY = useRef<number | null>(null);
  const nestedCanScrollDown = (target: EventTarget | null) => {
    const el = parentRef.current;
    let node = target instanceof Element ? target : null;
    while (node && node !== el) {
      if (
        node instanceof HTMLElement &&
        node.scrollHeight > node.clientHeight + 1 &&
        /^(auto|scroll|overlay)$/.test(getComputedStyle(node).overflowY) &&
        node.scrollTop + node.clientHeight < node.scrollHeight - 1
      )
        return true;
      node = node.parentElement;
    }
    return false;
  };

  const cancelForcedFollowForUserScroll = useCallback(() => {
    cancelPendingReplyFollow();
    anchorRestoration.cancel();
    if (shouldForceStickToBottom()) cancelPendingBottomScroll();
  }, [
    anchorRestoration,
    cancelPendingBottomScroll,
    cancelPendingReplyFollow,
    shouldForceStickToBottom,
  ]);
  const cancelForcedFollowOnKey = useBottomFollowKeyCancel(cancelForcedFollowForUserScroll);

  const wasFollowingBeforeContentChange = useCallback(
    (previousContentSize?: number) => {
      const el = parentRef.current;
      if (!el) return false;
      return wasAtBottomBeforeContentChange({
        previousContentSize: previousContentSize ?? previousContentSizeRef.current,
        scrollTop: el.scrollTop,
        clientHeight: el.clientHeight,
      });
    },
    [previousContentSizeRef],
  );

  const shouldAutoScrollNow = useCallback(
    (opts?: { previousContentSize?: number }) => {
      if (shouldForceStickToBottom()) return true;
      return shouldAutoScrollToBottom({
        followMode: followModeRef.current,
        isAtBottom:
          isAtBottomRef.current && wasFollowingBeforeContentChange(opts?.previousContentSize),
        forceStickToBottom: false,
      });
    },
    [followModeRef, isAtBottomRef, shouldForceStickToBottom, wasFollowingBeforeContentChange],
  );

  useViewChangeAutoFollow({
    changeKey: followOnViewChangeKey,
    messageCount: displayMessages.length,
    setAtBottom,
    setFollowMode,
    cancelAnchorRestoration: anchorRestoration.cancel,
    scheduleForceStickToBottom,
  });

  const scheduleScroll = useCallback(
    (fn: () => void) => {
      cancelScheduledScroll();
      scrollRafRef.current = window.requestAnimationFrame(() => {
        scrollRafRef.current = null;
        fn();
      });
    },
    [cancelScheduledScroll, scrollRafRef],
  );
  // The opening place waits a frame on its own: a scroll the list fires itself while opening,
  // such as the virtualizer applying its initial offset, cancels pending bottom scrolls only.
  const initialScrollRafRef = useRef<number | null>(null);
  const scheduleInitialScroll = useCallback((fn: () => void) => {
    if (initialScrollRafRef.current != null) {
      window.cancelAnimationFrame(initialScrollRafRef.current);
    }
    initialScrollRafRef.current = window.requestAnimationFrame(() => {
      initialScrollRafRef.current = null;
      fn();
    });
  }, []);
  useEffect(
    () => () => {
      if (initialScrollRafRef.current != null) {
        window.cancelAnimationFrame(initialScrollRafRef.current);
      }
    },
    [],
  );

  const scrollToIndexStable = useCallback(
    (idx: number) => {
      cancelPendingBottomScroll();
      const token = scrollTokenRef.current;
      const doScroll = () => {
        virtualizer.scrollToIndex(idx, { align: "center", behavior: "auto" });
      };
      doScroll();

      scrollRafRef.current = window.requestAnimationFrame(() => {
        scrollRafRef.current = null;
        if (scrollTokenRef.current !== token) return;
        doScroll();
      });
    },
    [cancelPendingBottomScroll, scrollRafRef, scrollTokenRef, virtualizer],
  );

  const {
    replyJumpHighlightId,
    replyJumpNotice,
    openReplyTarget: handleOpenReplyTarget,
  } = useReplyTargetNavigation({
    messages: displayMessages,
    shouldVirtualize,
    missingTargetMessage: t("replyTargetNotLoaded"),
    parentRef,
    onScrollTopChange: (top) => {
      lastScrollTopRef.current = top;
    },
    getMessageRowById,
    scrollToIndexStable,
    cancelPendingBottomScroll,
    setAtBottom,
    setFollowMode,
  });

  const handleScroll = useCallback(() => {
    setUnscrollable(false);
    const currentEl = parentRef.current;
    if (currentEl && !isContainerResizingRef.current) {
      const curTop = currentEl.scrollTop;
      const previousTop = lastScrollTopRef.current;
      const atBottom = checkIsAtBottom();
      const wasAtBottom = isAtBottomRef.current;
      setAtBottom(atBottom);
      const recentDown =
        scrollIntentIdRef.current !== consumedContextScrollIntentRef.current &&
        curTop > scrollIntentStartTopRef.current &&
        Date.now() - scrollIntentAtRef.current <= 2000;
      if (
        Date.now() - scrollIntentAtRef.current <= 2000 &&
        curTop < scrollIntentStartTopRef.current
      )
        awaitingLiveEndRef.current = false;
      if (
        recentDown &&
        atBottom &&
        contextWindow &&
        !onLoadNewerContext &&
        onReachedContextEndRef.current
      )
        awaitingLiveEndRef.current = true;
      const appendAnchor = recentDown ? captureContextAppend(curTop) : null;
      const loadingNewer = recentDown && notifyContextEnd(false);
      if (recentDown && !loadingNewer && onReachedContextEndRef.current) {
        const intentId = scrollIntentIdRef.current;
        // The virtualizer may not render its last row until after this scroll event.
        // Check again after measurement, without treating a layout scroll as reader intent.
        window.requestAnimationFrame(() =>
          window.requestAnimationFrame(() => {
            if (
              parentRef.current !== currentEl ||
              intentId !== scrollIntentIdRef.current ||
              intentId === consumedContextScrollIntentRef.current ||
              Date.now() - scrollIntentAtRef.current > 2000
            )
              return;
            const top = currentEl.scrollTop;
            const anchor = captureContextAppend(top);
            if (!notifyContextEnd(false)) return;
            consumedContextScrollIntentRef.current = intentId;
            prependCompensation.begin(anchor);
            lastScrollTopRef.current = top;
            setAtBottom(false);
            setFollowMode("detached");
            cancelPendingBottomScroll();
            onScrollChange?.(false);
          }),
        );
      }
      if (atBottom && !isLoadingNewerContext) {
        if (
          shouldPromoteScrollToFollow({
            followMode: followModeRef.current,
            previousTop,
            currentTop: curTop,
          })
        ) {
          setFollowMode("follow");
        }
      } else if (shouldForceStickToBottom()) {
        scrollToBottom({ force: true, requestToken: bottomScrollRequestTokenRef.current });
      } else {
        setFollowMode("detached");
        cancelPendingBottomScroll();
      }
      if (shouldNotifyScrollChange({ wasAtBottom, atBottom, showScrollButton, chatUnreadCount })) {
        onScrollChange?.(atBottom);
      }
      if (loadingNewer) {
        consumedContextScrollIntentRef.current = scrollIntentIdRef.current;
        if (appendAnchor) prependCompensation.begin(appendAnchor);
        lastScrollTopRef.current = curTop;
        setAtBottom(false);
        setFollowMode("detached");
        cancelPendingBottomScroll();
        onScrollChange?.(false);
      }
    }

    if (scrollRafScheduledRef.current) return;
    scrollRafScheduledRef.current = true;

    window.requestAnimationFrame(() => {
      scrollRafScheduledRef.current = false;

      const el = parentRef.current;
      if (!el) return;

      const topTriggerPx = HISTORY_TOP_TRIGGER_PX;
      const topRearmPx = HISTORY_TOP_REARM_PX;
      const curTop = el.scrollTop;
      const previousTop = lastScrollTopRef.current;
      const atBottom = checkIsAtBottom();
      if (
        shouldDetachChatFollowOnScroll({
          followMode: followModeRef.current,
          previousTop,
          currentTop: curTop,
          atBottom,
          isContainerResizing: isContainerResizingRef.current || shouldForceStickToBottom(),
          topLoadThresholdPx: topTriggerPx,
        })
      ) {
        setFollowMode("detached");
        cancelPendingBottomScroll();
      }
      lastScrollTopRef.current = curTop;

      if (atBottom && !isLoadingNewerContext && followModeRef.current === "detached") {
        if (
          shouldPromoteScrollToFollow({
            followMode: followModeRef.current,
            previousTop,
            currentTop: curTop,
          })
        ) {
          setFollowMode("follow");
        }
      }
      const wasAtBottom = isAtBottomRef.current;
      setAtBottom(atBottom);
      if (shouldNotifyScrollChange({ wasAtBottom, atBottom, showScrollButton, chatUnreadCount })) {
        onScrollChange?.(atBottom);
      }

      const isRestoringAnchor = anchorRestoration.isActive();
      if (!isRestoringAnchor) {
        const anchor = getAnchorSnapshot(curTop);
        if (anchor) {
          const snap: ChatScrollSnapshot = {
            coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
            mode: atBottom ? ("follow" as const) : followModeRef.current,
            anchorId: atBottom ? "" : anchor.anchorId,
            offsetPx: atBottom ? 0 : anchor.offsetPx,
            scrollTop: curTop,
            updatedAt: Date.now(),
          };
          latestSnapshotRef.current = snap;
          if (snapshotFlushTimerRef.current) window.clearTimeout(snapshotFlushTimerRef.current);
          snapshotFlushTimerRef.current = window.setTimeout(() => {
            snapshotFlushTimerRef.current = null;
            if (latestSnapshotRef.current) {
              writeEngagedSnapshot(withReadingFocus(latestSnapshotRef.current));
            }
          }, 300);
        }
      }

      // Top detection for loading more history.
      //
      // Use a hysteresis "arm/disarm" gate instead of relying on scroll direction.
      // This prevents repeated loads when the scroll position jitters near the top
      // (e.g. due to browser scroll anchoring or dynamic row measurement).
      if (!isRestoringAnchor && !shouldForceStickToBottom()) {
        topHistoryLoad.handleTopHistoryScroll({
          scrollTop: curTop,
          topTriggerPx,
          topRearmPx,
          hasMoreHistory,
          isLoadingHistory,
        });
      }
    });
  }, [
    cancelPendingBottomScroll,
    captureContextAppend,
    contextWindow,
    onLoadNewerContext,
    prependCompensation,
    anchorRestoration,
    bottomScrollRequestTokenRef,
    chatUnreadCount,
    checkIsAtBottom,
    getAnchorSnapshot,
    hasMoreHistory,
    followModeRef,
    isAtBottomRef,
    isContainerResizingRef,
    isLoadingHistory,
    isLoadingNewerContext,
    lastScrollTopRef,
    latestSnapshotRef,
    onScrollChange,
    notifyContextEnd,
    scrollIntentAtRef,
    scrollIntentStartTopRef,
    scrollIntentIdRef,
    scrollRafScheduledRef,
    scrollToBottom,
    shouldForceStickToBottom,
    setAtBottom,
    setFollowMode,
    showScrollButton,
    snapshotFlushTimerRef,
    topHistoryLoad,
    withReadingFocus,
    writeEngagedSnapshot,
  ]);

  // When switching views (group or window-mode), reset internal scroll bookkeeping.
  //
  // Important: this must run before the auto-scroll effects below, otherwise it may
  // cancel their scheduled scrolls (breaking deep-link jump precision).
  useEffect(() => {
    const prevKey = prevResetKeyRef.current;

    if (prevKey === resetKey) {
      return;
    }

    if (prevKey && latestSnapshotRef.current && engagedRef.current) {
      const prevGroupId = prevKey.split(":")[0];
      if (prevGroupId) {
        onScrollSnapshot?.(latestSnapshotRef.current, prevGroupId);
      }
    }

    prevResetKeyRef.current = resetKey;
    latestSnapshotRef.current = null;
    cancelPendingReplyFollow();

    scrollTokenRef.current += 1;
    const hasInitialJumpTarget = !!(initialScrollAnchorId || initialScrollTargetId);
    setAtBottom(!hasInitialJumpTarget);
    setFollowMode(hasInitialJumpTarget ? "detached" : "follow");
    // A view opening on a saved place tells the page it is away from the latest messages: the
    // view before may have left the page marked at the bottom, and a place restored from its
    // saved offset can skip the restore callback. Reaching the bottom is left to scrolling, so a
    // view whose target is still loading does not clear the unread count.
    if (hasInitialJumpTarget) onScrollChange?.(false);
    didInitialScrollRef.current = false;
    initialScrollRequestRef.current = "";
    initialScrollReentryDeadlineRef.current = Date.now() + 1_500;
    cancelPendingBottomScroll();
    if (snapshotFlushTimerRef.current) {
      window.clearTimeout(snapshotFlushTimerRef.current);
      snapshotFlushTimerRef.current = null;
    }
    topHistoryLoad.reset();
    lastScrollTopRef.current = 0;
    previousContentSizeRef.current = getCurrentContentSize();
    prevTailSnapshotRef.current = getChatTailSnapshot(
      displayMessages.length > 0
        ? getStableMessageKey(
            displayMessages[displayMessages.length - 1],
            displayMessages.length - 1,
          )
        : null,
      displayMessages.length,
    );
    prevTailMutationSnapshotRef.current = getChatTailMutationSnapshot(
      displayMessages.length > 0
        ? getStableMessageKey(
            displayMessages[displayMessages.length - 1],
            displayMessages.length - 1,
          )
        : null,
      "",
    );
  }, [
    cancelPendingBottomScroll,
    didInitialScrollRef,
    displayMessages,
    engagedRef,
    getCurrentContentSize,
    initialScrollAnchorId,
    initialScrollTargetId,
    initialScrollReentryDeadlineRef,
    initialScrollRequestRef,
    resetKey,
    lastScrollTopRef,
    latestSnapshotRef,
    onScrollChange,
    onScrollSnapshot,
    previousContentSizeRef,
    prevResetKeyRef,
    prevTailMutationSnapshotRef,
    prevTailSnapshotRef,
    cancelPendingReplyFollow,
    setAtBottom,
    setFollowMode,
    snapshotFlushTimerRef,
    scrollTokenRef,
    topHistoryLoad,
  ]);

  useInitialMessageScroll({
    messages: displayMessages,
    didInitialScrollRef,
    requestRef: initialScrollRequestRef,
    reentryDeadlineRef: initialScrollReentryDeadlineRef,
    targetId: initialScrollTargetId,
    anchorId: initialScrollAnchorId,
    anchorOffsetPx: initialScrollAnchorOffsetPx,
    targetOffsetPx: getReadingTopAnchorOffset(topInset),
    scheduleScroll: scheduleInitialScroll,
    beginAnchorRestoration: anchorRestoration.begin,
    setAtBottom,
    setFollowMode,
    scheduleForceStickToBottom: scheduleInitialFollow,
    onScrollSnapshot: writeEngagedSnapshot,
    onRestoreAwayFromBottom: notifyRestoredAwayFromBottom,
    isEngaged,
    highlightAnchor: highlightInitialScrollAnchor,
    onHighlightAnchor: markArrival,
  });

  // The explicit context target must be positioned before it moves to live chat. Do not move a
  // reader who has already acted in the context, even if the live tail finishes loading later.
  useEffect(() => {
    if (
      (!onContextReady && !onReachedContextEnd) ||
      isLoadingHistory ||
      !initialScrollTargetId ||
      !displayMessages.length
    )
      return;
    let secondFrame = 0;
    const firstFrame = requestAnimationFrame(() => {
      secondFrame = requestAnimationFrame(() => {
        const el = parentRef.current;
        const target = getMessageRowById(initialScrollTargetId);
        if (!engagedRef.current && el && target) {
          const box = el.getBoundingClientRect();
          const row = target.getBoundingClientRect();
          if (row.bottom > box.top && row.top < box.bottom && onContextReadyRef.current) {
            onContextReadyRef.current();
            return;
          }
        }
        notifyContextEnd(true);
      });
    });
    return () => {
      cancelAnimationFrame(firstFrame);
      cancelAnimationFrame(secondFrame);
    };
  }, [
    displayMessages.length,
    engagedRef,
    getMessageRowById,
    initialScrollTargetId,
    isLoadingHistory,
    notifyContextEnd,
    onContextReady,
    onReachedContextEnd,
  ]);

  // If the live tail was still loading when the reader reached the end, complete the handoff
  // when it arrives. A reader who moves away from the end keeps their place.
  useEffect(() => {
    if (!awaitingLiveEndRef.current || !contextWindow || onLoadNewerContext || !onReachedContextEnd)
      return;
    if (!checkIsAtBottom()) {
      awaitingLiveEndRef.current = false;
      return;
    }
    notifyContextEnd(false);
  }, [checkIsAtBottom, contextWindow, notifyContextEnd, onLoadNewerContext, onReachedContextEnd]);

  // If a target was opened at the live end, resume following even when the list was too short
  // to emit a scroll event. A deliberately detached reader keeps their position.
  useEffect(() => {
    if (!liveJumpActive || isLoadingHistory || !displayMessages.length) return;
    let secondFrame = 0;
    const firstFrame = requestAnimationFrame(() => {
      secondFrame = requestAnimationFrame(() => {
        if (engagedRef.current || !parentRef.current?.clientHeight || !checkIsAtBottom()) return;
        const tailId = String(displayMessages.at(-1)?.id || "");
        const tail = getMessageRowById(tailId);
        const box = parentRef.current.getBoundingClientRect();
        if (!tail || tail.getBoundingClientRect().top >= box.bottom) return;
        anchorRestoration.cancel();
        setAtBottom(true);
        setFollowMode("follow");
        onScrollChange?.(true);
      });
    });
    return () => {
      cancelAnimationFrame(firstFrame);
      cancelAnimationFrame(secondFrame);
    };
  }, [
    anchorRestoration,
    checkIsAtBottom,
    displayMessages,
    engagedRef,
    getMessageRowById,
    isLoadingHistory,
    liveJumpActive,
    onScrollChange,
    setAtBottom,
    setFollowMode,
  ]);

  useSendScrollRequestLifecycle({
    activeRequestRef: activeSendScrollRequestRef,
    groupId,
    viewKey: ownerViewKey,
    request: sendScrollRequest,
    onConsumed: onSendScrollRequestConsumed,
    // Sending moves the reader to the latest message, which is acting in this view.
    onApplied: markEngaged,
    setAtBottom,
    setFollowMode,
    requestTokenRef: bottomScrollRequestTokenRef,
    scheduleScroll,
    scrollToBottom,
    cancelPendingBottomScroll,
  });

  useMessageTailAutoFollow({
    messages: displayMessages,
    didInitialScrollRef,
    previousTailRef: prevTailSnapshotRef,
    previousMutationRef: prevTailMutationSnapshotRef,
    previousContentSizeRef,
    getCurrentContentSize,
    isLoadingHistory,
    shouldAutoScroll: shouldAutoScrollNow,
    scheduleScroll,
    scrollToBottom: followLatestMessage,
    onTailChange: cancelPendingReplyFollow,
    onFollowAppend: considerPendingReply,
  });

  useEffect(() => {
    const scrollEl = parentRef.current;
    const observedEl = contentRef.current;
    if (!scrollEl || !observedEl || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(([entry]) => {
      // Observe the message content layer rather than the scroll container.
      // Images, streaming text, and expanded attachment lists change content height
      // without changing the container size; observing only the container misses bottom-follow updates.
      lastScrollTopRef.current = scrollEl.scrollTop;
      const previousContentSize = previousContentSizeRef.current;
      previousContentSizeRef.current = getCurrentContentSize();
      const width = entry.contentRect.width;
      const widthChanged =
        width > 0 && contentWidthRef.current !== null && contentWidthRef.current !== width;
      if (width > 0) contentWidthRef.current = width;
      // Virtual rows can finish measuring after the scroll event without moving
      // scrollTop. Refresh the anchor then, before a later width change uses it.
      if (
        width > 0 &&
        !widthChanged &&
        followModeRef.current === "detached" &&
        !anchorRestoration.isActive()
      ) {
        latestSnapshotRef.current = captureScrollSnapshotRef.current();
      }
      const snapshot = latestSnapshotRef.current;
      // Width changes reflow earlier messages, so the old pixel scroll offset
      // no longer identifies the message being read. Reuse its saved anchor.
      if (
        widthChanged &&
        followModeRef.current === "detached" &&
        !shouldAutoScrollNow({ previousContentSize }) &&
        !anchorRestoration.isActive() &&
        snapshot?.mode === "detached" &&
        snapshot.anchorId
      ) {
        anchorRestoration.begin({ anchorId: snapshot.anchorId, offsetPx: snapshot.offsetPx });
      }
      anchorRestoration.correct();

      if (shouldAutoScrollNow({ previousContentSize })) {
        scheduleScroll(() => {
          if (!shouldAutoScrollNow({ previousContentSize })) return;
          followLatestMessage();
        });
      }

      window.requestAnimationFrame(() => {
        lastScrollTopRef.current = scrollEl.scrollTop;
      });
    });
    observer.observe(observedEl);
    return () => observer.disconnect();
  }, [
    anchorRestoration,
    followModeRef,
    getCurrentContentSize,
    lastScrollTopRef,
    latestSnapshotRef,
    previousContentSizeRef,
    scheduleScroll,
    followLatestMessage,
    shouldAutoScrollNow,
  ]);

  // A change of body font or width reflows every message, so the
  // observer above would re-anchor on whichever message now sits at the top.
  // The store notifies before any re-render, font load, or observer callback,
  // while the saved anchor still names the message being read. A reader who
  // follows the latest message keeps following it, as after a view change.
  useEffect(
    () =>
      subscribeUiStylePrefs(() => {
        if (followModeRef.current === "follow") {
          scheduleForceStickToBottom();
          return;
        }
        if (anchorRestoration.extend()) return;
        const snapshot = latestSnapshotRef.current;
        if (snapshot?.mode !== "detached" || !snapshot.anchorId) return;
        anchorRestoration.begin({ anchorId: snapshot.anchorId, offsetPx: snapshot.offsetPx });
      }),
    [anchorRestoration, followModeRef, latestSnapshotRef, scheduleForceStickToBottom],
  );

  useLayoutEffect(() => {
    onScrollSnapshotRef.current = onScrollSnapshot;
  }, [onScrollSnapshot]);
  const contextWindowRef = useRef(!!contextWindow);
  useLayoutEffect(() => {
    contextWindowRef.current = !!contextWindow;
  }, [contextWindow]);

  useLayoutEffect(() => {
    return () => {
      if (remeasureRafRef.current != null) {
        window.cancelAnimationFrame(remeasureRafRef.current);
        remeasureRafRef.current = null;
      }
      prependCompensation.cancelCorrection();
    };
  }, [prependCompensation]);

  useLayoutEffect(() => {
    return () => {
      if (snapshotFlushTimerRef.current) {
        window.clearTimeout(snapshotFlushTimerRef.current);
        snapshotFlushTimerRef.current = null;
      }
      const snapshot = captureScrollSnapshotRef.current() || latestSnapshotRef.current;
      // A message window's place is not the Group's: leaving it returns to the place the
      // conversation had before it opened, or to the newest message.
      if (snapshot && engagedRef.current && !contextWindowRef.current) {
        const currentGroupId = resetKey.split(":")[0];
        if (currentGroupId) {
          onScrollSnapshotRef.current?.(snapshot, currentGroupId);
        }
      }
      if (scrollRef) {
        scrollRef.current = null;
      }
    };
  }, [engagedRef, latestSnapshotRef, resetKey, scrollRef, snapshotFlushTimerRef]);

  useLayoutEffect(() => {
    topHistoryLoad.applyPendingPrependCompensation({
      isLoadingHistory: isLoadingHistory || !!isLoadingNewerContext,
    });
  }, [displayMessages, isLoadingHistory, isLoadingNewerContext, topHistoryLoad]);

  const effectiveHighlightEventId = replyJumpHighlightId || arrivalHighlightId || highlightEventId;
  const showManualHistory =
    !contextWindow && !!onLoadMore && hasMoreHistory && (hasActiveFilters || shortForHistory);
  const showHistoryStatus =
    showManualHistory || isLoadingHistory || (!contextWindow && !hasMoreHistory);
  const nonVirtualTopMargin = getNonVirtualMessageListTopMargin({ topInset });
  const scrollLatestLabel = t("scrollToBottom");
  const unreadDisplayCount = chatUnreadCount > 99 ? "99+" : String(chatUnreadCount);
  const unreadLabel =
    chatUnreadCount > 0
      ? t("scrollToBottomUnread", { count: chatUnreadCount, displayCount: unreadDisplayCount })
      : "";

  return (
    <div className="relative flex-1 min-h-0 flex flex-col overflow-hidden">
      <div
        ref={(el) => {
          parentRef.current = el;
          if (scrollRef) scrollRef.current = el;
        }}
        className={classNames(
          "chat-reading-scroller flex-1 min-h-0 overflow-auto px-4 py-4 relative",
          className,
        )}
        style={{
          overflowAnchor: "none",
          scrollbarGutter: "stable both-edges",
          scrollPaddingTop: topInset || undefined,
        }}
        onKeyDownCapture={(event) => {
          cancelForcedFollowOnKey(event);
          if (event.target === event.currentTarget) {
            if (
              event.key === "Home" ||
              event.key === "PageUp" ||
              event.key === "ArrowUp" ||
              (event.key === " " && event.shiftKey)
            )
              awaitingLiveEndRef.current = false;
            if (
              event.key === "End" ||
              event.key === "PageDown" ||
              event.key === "ArrowDown" ||
              (event.key === " " && !event.shiftKey)
            )
              continueContextAtBottom();
          }
        }}
        onFocusCapture={cancelForcedFollowForUserScroll}
        onWheel={(event) => {
          cancelForcedFollowForUserScroll();
          if (event.deltaY < 0) awaitingLiveEndRef.current = false;
          if (event.deltaY > 0 && !nestedCanScrollDown(event.target)) continueContextAtBottom();
        }}
        onPointerDown={(event) => {
          if (event.target === event.currentTarget) cancelForcedFollowForUserScroll();
          else {
            cancelPendingReplyFollow();
            anchorRestoration.cancel();
          }
        }}
        onTouchStart={(event) => {
          cancelForcedFollowForUserScroll();
          contextTouchStartY.current = event.touches.length === 1 ? event.touches[0].clientY : null;
        }}
        onTouchMove={(event) => {
          const startY = contextTouchStartY.current;
          if (startY != null && event.touches.length === 1) {
            if (event.touches[0].clientY > startY + 8) awaitingLiveEndRef.current = false;
            if (event.touches[0].clientY < startY - 8 && !nestedCanScrollDown(event.target))
              continueContextAtBottom();
          }
        }}
        onScroll={displayMessages.length > 0 ? handleScroll : undefined}
        role="log"
        tabIndex={-1}
        aria-label="Chat messages"
      >
        {displayMessages.length === 0 ? (
          <VirtualMessageListEmptyState
            isLoadingHistory={isLoadingHistory}
            hasMoreHistory={hasMoreHistory}
            isFilteredEmpty={isFilteredEmpty}
            emptyMessage={emptyMessage}
            onLoadMore={onLoadMore}
          />
        ) : (
          <>
            {showHistoryStatus && (
              <div
                className="pointer-events-none absolute inset-x-0 z-10 flex justify-center py-3"
                style={{ top: topInset }}
              >
                {showManualHistory ? (
                  <button
                    type="button"
                    data-load-older-history
                    aria-disabled={isLoadingHistory}
                    className="pointer-events-auto glass-panel flex min-h-9 items-center gap-2 rounded-full px-3 py-1.5 text-xs text-[var(--color-text-secondary)] shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45"
                    onClick={(event) => {
                      if (isLoadingHistory) return;
                      // The button can disappear when the last page arrives. Keep keyboard
                      // focus in the list instead of letting it fall back to the document.
                      if (event.detail === 0 && document.activeElement === event.currentTarget)
                        parentRef.current?.focus({ preventScroll: true });
                      markEngaged();
                      topHistoryLoad.requestTopHistoryLoad({
                        scrollTop: parentRef.current?.scrollTop ?? 0,
                        hasMoreHistory,
                        isLoadingHistory,
                      });
                    }}
                  >
                    {isLoadingHistory ? (
                      <>
                        <LcdSpinner className="h-4 w-4" />
                        {t("loadingHistory", { defaultValue: "Loading..." })}
                      </>
                    ) : (
                      t("loadOlderResults")
                    )}
                  </button>
                ) : isLoadingHistory ? (
                  <div className="glass-panel flex items-center gap-2 rounded-full px-3 py-1.5 text-[var(--color-text-secondary)] shadow-md">
                    <LcdSpinner className="h-4 w-4" />
                    <span className="text-xs">
                      {t("loadingHistory", { defaultValue: "Loading..." })}
                    </span>
                  </div>
                ) : (
                  <div className="glass-panel rounded-full px-3 py-1.5 text-xs text-[var(--color-text-tertiary)] shadow-sm">
                    {t("noMoreMessages", { defaultValue: "No more messages" })}
                  </div>
                )}
              </div>
            )}

            {replyJumpNotice ? (
              <div
                className="pointer-events-none absolute inset-x-0 z-20 flex justify-center px-4"
                style={{ top: topInset + 48 }}
              >
                <div className="glass-panel rounded-full px-3 py-1 text-xs text-[var(--color-text-secondary)] shadow-sm">
                  {replyJumpNotice}
                </div>
              </div>
            ) : null}

            <MessageRows
              messages={displayMessages}
              shouldVirtualize={shouldVirtualize}
              virtualItems={virtualizer.getVirtualItems()}
              totalVirtualSize={virtualizer.getTotalSize()}
              nonVirtualTopMargin={nonVirtualTopMargin}
              contentRef={contentRef}
              messageTextById={messageTextById}
              actorById={actorById}
              actors={actors}
              agentStateById={agentStateById}
              displayNameMap={displayNameMap}
              taskById={taskById}
              isDark={isDark}
              readOnly={readOnly}
              groupId={groupId}
              groupLabelById={groupLabelById}
              webModelDeliveryStatusByEventId={webModelDeliveryStatusByEventId}
              effectiveHighlightEventId={effectiveHighlightEventId}
              onReply={onReply}
              onShowRecipients={onShowRecipients}
              onViewContext={onViewContext}
              onCopyLink={onCopyLink}
              onTogglePin={onTogglePin}
              onRelay={onRelay}
              onOpenSource={onOpenSource}
              onOpenPresentationRef={onOpenPresentationRef}
              onOpenTaskRef={onOpenTaskRef}
              onOpenReplyTarget={handleOpenReplyTarget}
              measureElement={measureElement}
            />
            {onLoadNewerContext && (
              <div className="chat-reading-width flex justify-center py-3">
                <button
                  type="button"
                  className="min-h-9 text-xs text-[var(--color-text-secondary)] underline"
                  disabled={isLoadingNewerContext}
                  onClick={() => {
                    const top = parentRef.current?.scrollTop ?? 0;
                    prependCompensation.begin(captureContextAppend(top));
                    lastScrollTopRef.current = top;
                    setAtBottom(false);
                    setFollowMode("detached");
                    cancelPendingBottomScroll();
                    onLoadNewerContext();
                  }}
                >
                  {t(
                    isLoadingNewerContext
                      ? "loadingNewerContext"
                      : newerContextError
                        ? "retryNewerContext"
                        : "loadNewerContext",
                  )}
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {/* Float over the log so returning to the latest message takes no reading height. */}
      {!readOnly && showScrollButton && !(unscrollable && chatUnreadCount <= 0) && (
        <button
          type="button"
          data-scroll-latest
          onClick={(event) => {
            // The button disappears after activation. Keep its focus in the log
            // instead of dropping it onto the body when a context view is replaced.
            // Focus first: entering the log cancels any earlier forced follow.
            if (
              (event.detail === 0 || contextWindow) &&
              document.activeElement === event.currentTarget
            ) {
              parentRef.current?.focus({ preventScroll: true });
            }
            markEngaged();
            cancelPendingReplyFollow();
            anchorRestoration.cancel();
            if (contextWindow) {
              onScrollButtonClick();
              return;
            }
            scrollToBottom({ force: true });
            onScrollButtonClick();
            const appendAnchor = captureContextAppend(parentRef.current?.scrollTop ?? 0);
            if (onReachedContextEndRef.current?.(false, true) === true) {
              prependCompensation.begin(appendAnchor);
              lastScrollTopRef.current = parentRef.current?.scrollTop ?? 0;
              setAtBottom(false);
              setFollowMode("detached");
              cancelPendingBottomScroll();
            }
          }}
          aria-label={unreadLabel ? `${scrollLatestLabel} · ${unreadLabel}` : scrollLatestLabel}
          title={unreadLabel ? `${scrollLatestLabel} · ${unreadLabel}` : scrollLatestLabel}
        >
          <ArrowDownIcon className="h-5 w-5" aria-hidden="true" />
          {unreadLabel ? (
            <span data-scroll-latest-count title={unreadLabel} aria-hidden="true">
              {unreadDisplayCount}
            </span>
          ) : null}
        </button>
      )}
    </div>
  );
};

export const VirtualMessageList = memo(function VirtualMessageList(props: VirtualMessageListProps) {
  const resetKey = props.viewKey ?? props.groupId;
  // Group/window switches must remount the virtualizer instance. Reusing a
  // single instance across transcripts lets measurement/order caches bleed
  // into the next view, which is worse than a brief re-measure on mount.
  return <VirtualMessageListInner key={resetKey} {...props} resetKey={resetKey} />;
});
