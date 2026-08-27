import { memo, useRef, useEffect, useLayoutEffect, useCallback } from "react";
import { measureElement as measureVirtualElement, useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import { ArrowDownIcon } from "./Icons";
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
import { getNonVirtualMessageListTopMargin } from "./virtualMessageListLayout";
import { useVirtualMessageMetadata } from "./virtualMessageList/useVirtualMessageMetadata";
import type { VirtualMessageListProps } from "./virtualMessageList/types";
import { useReplyTargetNavigation } from "./virtualMessageList/useReplyTargetNavigation";
import { MessageRows } from "./virtualMessageList/MessageRows";
import { useVirtualScrollState } from "./virtualMessageList/useVirtualScrollState";
import { useScrollAnchorRestoration as useAnchorRestoration } from "./virtualMessageList/useScrollAnchorRestoration";
import { useInitialMessageScroll } from "./virtualMessageList/useInitialMessageScroll";
import { useMessageTailAutoFollow } from "./virtualMessageList/useMessageTailAutoFollow";
import { cacheMessageRowHeight } from "./virtualMessageList/rowHeightCache";
import {
  getMessageAnchorOffset,
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
  highlightEventId,
  className,
  topInsetPx = 0,
  scrollRef,
  onReply,
  onShowRecipients,
  onCopyLink,
  onCopyContent,
  onRelay,
  onOpenSource,
  onOpenPresentationRef,
  onOpenTaskRef,
  showScrollButton,
  onScrollButtonClick,
  chatUnreadCount,
  onScrollChange,
  onScrollSnapshot,
  sendScrollRequest,
  onSendScrollRequestConsumed,
  isLoadingHistory = false,
  hasMoreHistory = true,
  isFilteredEmpty = false,
  onLoadMore,
  resetKey,
}: VirtualMessageListInnerProps) {
  const { t } = useTranslation("chat");
  const parentRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const onScrollSnapshotRef = useRef(onScrollSnapshot);
  const captureScrollSnapshotRef = useRef<() => ChatScrollSnapshot | null>(() => null);
  const remeasureRafRef = useRef<number | null>(null);
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
        cacheMessageRowHeight(resetKey, getStableMessageKey(displayMessages[index], index), height);
      }
      return height;
    },
    initialOffset: Math.max(0, Number(initialScrollOffsetPx) || 0),
    overscan: VIRTUAL_OVERSCAN_ROWS,
    paddingStart: 72 + topInset,
  });

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

  const setFollowMode = useCallback(
    (next: ChatFollowMode) => {
      followModeRef.current = next;
    },
    [followModeRef],
  );

  const notifyRestoredAwayFromBottom = useCallback(() => {
    onScrollChange?.(false);
  }, [onScrollChange]);

  const detachFollowModeForHistoryLoad = useCallback(() => {
    setFollowMode("detached");
  }, [setFollowMode]);

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
        const offsetInfo = virtualizer.getOffsetForIndex(idx, "start");
        if (offsetInfo) {
          virtualizer.scrollToOffset(getScrollOffsetForMessageAnchor(offsetInfo[0], offsetPx), {
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

  const prependCompensation = usePrependCompensationController({
    parentRef,
    lastScrollTopRef,
    getMessageRowById,
    isVirtualized: shouldVirtualize,
    scrollToVirtualOffset,
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

  const captureCurrentScrollSnapshot = useCallback((): ChatScrollSnapshot | null => {
    const el = parentRef.current;
    if (!el || displayMessages.length <= 0) return latestSnapshotRef.current;
    if (checkIsAtBottom()) {
      return {
        coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
        mode: "follow",
        anchorId: "",
        offsetPx: 0,
        scrollTop: el.scrollTop,
        updatedAt: Date.now(),
      };
    }
    const anchor = getAnchorSnapshot(el.scrollTop);
    if (!anchor) return latestSnapshotRef.current;
    return {
      coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
      mode: "detached",
      anchorId: anchor.anchorId,
      offsetPx: anchor.offsetPx,
      scrollTop: el.scrollTop,
      updatedAt: Date.now(),
    };
  }, [checkIsAtBottom, displayMessages.length, getAnchorSnapshot, latestSnapshotRef]);

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
        const offsetInfo = virtualizer.getOffsetForIndex(idx, "start");
        if (!offsetInfo) {
          virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
          return true;
        }
        virtualizer.scrollToOffset(getScrollOffsetForMessageAnchor(offsetInfo[0], offsetPx), {
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

  const topHistoryLoad = useTopHistoryLoadCoordinator({
    compensation: prependCompensation,
    getAnchorSnapshot,
    getAnchorTop,
    getCurrentContentSize,
    scrollToMessageAnchor,
    cancelPendingBottomScroll,
    detachFollowMode: detachFollowModeForHistoryLoad,
    markAwayFromBottom: markAwayFromBottomForHistoryLoad,
    onLoadMore,
  });

  const cancelForcedFollowForUserScroll = useCallback(() => {
    anchorRestoration.cancel();
    if (shouldForceStickToBottom()) cancelPendingBottomScroll();
  }, [anchorRestoration, cancelPendingBottomScroll, shouldForceStickToBottom]);
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
    const currentEl = parentRef.current;
    if (currentEl && !isContainerResizingRef.current) {
      const curTop = currentEl.scrollTop;
      const previousTop = lastScrollTopRef.current;
      const atBottom = checkIsAtBottom();
      const wasAtBottom = isAtBottomRef.current;
      setAtBottom(atBottom);
      if (atBottom) {
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
    }

    if (scrollRafScheduledRef.current) return;
    scrollRafScheduledRef.current = true;

    window.requestAnimationFrame(() => {
      scrollRafScheduledRef.current = false;

      const el = parentRef.current;
      if (!el) return;

      const topTriggerPx = 80;
      const topRearmPx = 240;
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

      if (atBottom && followModeRef.current === "detached") {
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
              onScrollSnapshot?.(latestSnapshotRef.current);
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
    lastScrollTopRef,
    latestSnapshotRef,
    onScrollChange,
    onScrollSnapshot,
    scrollRafScheduledRef,
    scrollToBottom,
    shouldForceStickToBottom,
    setAtBottom,
    setFollowMode,
    showScrollButton,
    snapshotFlushTimerRef,
    topHistoryLoad,
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

    if (prevKey && latestSnapshotRef.current) {
      const prevGroupId = prevKey.split(":")[0];
      if (prevGroupId) {
        onScrollSnapshot?.(latestSnapshotRef.current, prevGroupId);
      }
    }

    prevResetKeyRef.current = resetKey;
    latestSnapshotRef.current = null;

    scrollTokenRef.current += 1;
    const hasInitialJumpTarget = !!(initialScrollAnchorId || initialScrollTargetId);
    setAtBottom(!hasInitialJumpTarget);
    setFollowMode(hasInitialJumpTarget ? "detached" : "follow");
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
    getCurrentContentSize,
    initialScrollAnchorId,
    initialScrollTargetId,
    initialScrollReentryDeadlineRef,
    initialScrollRequestRef,
    resetKey,
    lastScrollTopRef,
    latestSnapshotRef,
    onScrollSnapshot,
    previousContentSizeRef,
    prevResetKeyRef,
    prevTailMutationSnapshotRef,
    prevTailSnapshotRef,
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
    shouldVirtualize,
    scheduleScroll,
    scrollToIndex: scrollToIndexStable,
    scrollToMessageAnchor,
    beginAnchorRestoration: anchorRestoration.begin,
    setAtBottom,
    setFollowMode,
    scheduleForceStickToBottom,
    onScrollSnapshot,
    onRestoreAwayFromBottom: notifyRestoredAwayFromBottom,
  });

  useSendScrollRequestLifecycle({
    activeRequestRef: activeSendScrollRequestRef,
    groupId,
    viewKey: ownerViewKey,
    request: sendScrollRequest,
    onConsumed: onSendScrollRequestConsumed,
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
    scrollToBottom,
  });

  useEffect(() => {
    const scrollEl = parentRef.current;
    const observedEl = contentRef.current;
    if (!scrollEl || !observedEl || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(() => {
      // Observe the message content layer rather than the scroll container.
      // Images, streaming text, and expanded attachment lists change content height
      // without changing the container size; observing only the container misses bottom-follow updates.
      lastScrollTopRef.current = scrollEl.scrollTop;
      const previousContentSize = previousContentSizeRef.current;
      previousContentSizeRef.current = getCurrentContentSize();
      anchorRestoration.correct();

      if (shouldAutoScrollNow({ previousContentSize })) {
        scheduleScroll(() => {
          if (!shouldAutoScrollNow({ previousContentSize })) return;
          scrollToBottom();
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
    getCurrentContentSize,
    lastScrollTopRef,
    previousContentSizeRef,
    scheduleScroll,
    scrollToBottom,
    shouldAutoScrollNow,
  ]);

  useLayoutEffect(() => {
    onScrollSnapshotRef.current = onScrollSnapshot;
  }, [onScrollSnapshot]);

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
      if (snapshot) {
        const currentGroupId = resetKey.split(":")[0];
        if (currentGroupId) {
          onScrollSnapshotRef.current?.(snapshot, currentGroupId);
        }
      }
      if (scrollRef) {
        scrollRef.current = null;
      }
    };
  }, [latestSnapshotRef, resetKey, scrollRef, snapshotFlushTimerRef]);

  useLayoutEffect(() => {
    topHistoryLoad.applyPendingPrependCompensation({ isLoadingHistory });
  }, [displayMessages, isLoadingHistory, topHistoryLoad]);

  const effectiveHighlightEventId = replyJumpHighlightId || highlightEventId;
  const showHistoryStatus = isLoadingHistory || (!hasMoreHistory && !isLoadingHistory);
  const nonVirtualTopMargin = getNonVirtualMessageListTopMargin({ topInset, showHistoryStatus });

  return (
    <div className="relative flex-1 min-h-0 flex flex-col">
      <div
        ref={(el) => {
          parentRef.current = el;
          if (scrollRef) scrollRef.current = el;
        }}
        className={classNames("flex-1 min-h-0 overflow-auto px-4 py-4 relative", className)}
        style={{ overflowAnchor: "none" }}
        onKeyDownCapture={cancelForcedFollowOnKey}
        onWheel={cancelForcedFollowForUserScroll}
        onPointerDown={(event) => {
          if (event.target === event.currentTarget) cancelForcedFollowForUserScroll();
          else anchorRestoration.cancel();
        }}
        onTouchStart={cancelForcedFollowForUserScroll}
        onScroll={displayMessages.length > 0 ? handleScroll : undefined}
        role="log"
        aria-label="Chat messages"
      >
        {displayMessages.length === 0 ? (
          <VirtualMessageListEmptyState
            isLoadingHistory={isLoadingHistory}
            hasMoreHistory={hasMoreHistory}
            isFilteredEmpty={isFilteredEmpty}
            onLoadMore={onLoadMore}
          />
        ) : (
          <>
            {showHistoryStatus && (
              <div
                className="pointer-events-none absolute inset-x-0 z-10 flex justify-center py-3"
                style={{ top: topInset }}
              >
                {isLoadingHistory ? (
                  <div className="glass-panel flex items-center gap-2 rounded-full px-3 py-1.5 text-[var(--color-text-secondary)] shadow-md">
                    <div className="animate-spin w-4 h-4 border-2 border-current border-t-transparent rounded-full" />
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
              onCopyLink={onCopyLink}
              onCopyContent={onCopyContent}
              onRelay={onRelay}
              onOpenSource={onOpenSource}
              onOpenPresentationRef={onOpenPresentationRef}
              onOpenTaskRef={onOpenTaskRef}
              onOpenReplyTarget={handleOpenReplyTarget}
              measureElement={measureElement}
            />
          </>
        )}
      </div>

      {/* Scroll Button — positioned outside scrollable container for correct viewport anchoring */}
      {!readOnly && showScrollButton && (
        <button
          className="glass-panel absolute bottom-6 right-5 z-30 rounded-full p-3 shadow-xl transition-all duration-200 hover:shadow-2xl hover:scale-105 active:scale-95 animate-scale-in text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
          onClick={() => {
            scrollToBottom({ force: true });
            onScrollButtonClick();
          }}
          aria-label={t("scrollToBottom", { defaultValue: "Scroll to bottom" })}
        >
          <ArrowDownIcon className="w-5 h-5" aria-hidden="true" />
          {chatUnreadCount > 0 && (
            <span className="absolute -top-1.5 -right-1.5 inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded-full bg-indigo-500 px-1 text-[10px] font-bold text-white shadow-sm">
              {chatUnreadCount > 99 ? "99+" : chatUnreadCount}
            </span>
          )}
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
