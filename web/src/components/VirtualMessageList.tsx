import { memo, useRef, useEffect, useLayoutEffect, useCallback, useMemo, useState } from "react";
import type { MutableRefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTranslation } from "react-i18next";
import { LedgerEvent, Actor, AgentState, PresentationMessageRef, TaskMessageRef, Task, ChatMessageData } from "../types";
import { ArrowDownIcon, MessageSquareTextIcon } from "./Icons";
import { MessageBubble } from "./MessageBubble";
import { useActorDisplayNameMap } from "../hooks/useActorDisplayName";
import {
  getChatTailMutationSnapshot,
  getChatTailSnapshot,
} from "../utils/chatAutoFollow";
import { estimateMessageRowHeight } from "./messageBubble/estimate";
import type { ChatFollowMode } from "../stores/useUIStore";
import {
  getAutoFollowTrigger,
  getStableMessageKey,
  shouldAutoScrollToBottom,
  shouldDetachChatFollowOnScroll,
  shouldNotifyScrollChange,
  shouldRunScheduledBottomScroll,
  shouldUseVirtualizedMessageList,
} from "./virtualMessageListHelpers";
import { classNames } from "../utils/classNames";
import type { WebModelDeliveryStatus } from "../utils/webModelDeliveryStatus";

function shouldCollapseMessageHeader(previousMessage: LedgerEvent | undefined, message: LedgerEvent | undefined): boolean {
  if (!previousMessage || !message) return false;
  if (previousMessage.kind !== "chat.message" || message.kind !== "chat.message") return false;

  const prevBy = String(previousMessage.by || "").trim();
  const currBy = String(message.by || "").trim();
  if (!prevBy || !currBy || prevBy !== currBy) return false;

  const prevData = previousMessage.data as ChatMessageData | undefined;
  const currData = message.data as ChatMessageData | undefined;

  // Do not collapse if either message has attention priority or requires a reply
  if (prevData?.priority === "attention" || currData?.priority === "attention") return false;
  if (prevData?.reply_required || currData?.reply_required) return false;

  // Do not collapse if the message is a reply targeting another message
  if (currData?.reply_to) return false;

  if (!previousMessage.ts || !message.ts) return false;

  try {
    const prevTime = new Date(previousMessage.ts).getTime();
    const currTime = new Date(message.ts).getTime();
    if (isNaN(prevTime) || isNaN(currTime)) return false;

    // Collapse if sent within 3 minutes of the previous message
    const diffMs = Math.abs(currTime - prevTime);
    return diffMs < 3 * 60 * 1000;
  } catch {
    return false;
  }
}

function getMessageRowGrouping(previousMessage: LedgerEvent | undefined, message: LedgerEvent | undefined): {
  collapseHeader: boolean;
  compactSpacing: boolean;
} {
  const collapseHeader = shouldCollapseMessageHeader(previousMessage, message);
  return {
    collapseHeader,
    compactSpacing: collapseHeader,
  };
}

export interface VirtualMessageListProps {
  messages: LedgerEvent[];
  actors: Actor[];
  agentStates: AgentState[];
  taskById: Map<string, Task>;
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  webModelDeliveryStatusByEventId?: Record<string, WebModelDeliveryStatus>;
  viewKey?: string;
  initialScrollTargetId?: string;
  initialScrollAnchorId?: string;
  initialScrollAnchorOffsetPx?: number;
  highlightEventId?: string;
  className?: string;
  topInsetPx?: number;
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onReply: (ev: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  onCopyContent?: (ev: LedgerEvent) => void;
  onRelay?: (ev: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  showScrollButton: boolean;
  onScrollButtonClick: () => void;
  chatUnreadCount: number;
  onScrollChange?: (isAtBottom: boolean) => void;
  onScrollSnapshot?: (snap: { mode: ChatFollowMode; anchorId: string; offsetPx: number; updatedAt: number }, groupId?: string) => void;
  forceStickToBottomToken?: number;
  // History loading
  isLoadingHistory?: boolean;
  hasMoreHistory?: boolean;
  onLoadMore?: () => void;
}

type VirtualMessageListInnerProps = VirtualMessageListProps & {
  resetKey: string;
};

type VirtualMessageRowProps = {
  virtualRow: { key: React.Key; index: number; start: number };
  message: LedgerEvent;
  collapseHeader?: boolean;
  compactSpacing?: boolean;
  actorById: Map<string, Actor>;
  actors: Actor[];
  displayNameMap: Map<string, string>;
  agentState: AgentState | null;
  taskById: Map<string, Task>;
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  webModelDeliveryStatus?: WebModelDeliveryStatus;
  highlightEventId?: string;
  onReply: (ev: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  onCopyContent?: (ev: LedgerEvent) => void;
  onRelay?: (ev: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  onOpenReplyTarget?: (replyToEventId: string) => void;
  measureElement: (node: Element | null) => void;
};

const VirtualMessageRow = memo(function VirtualMessageRow({
  virtualRow,
  message,
  collapseHeader,
  compactSpacing,
  actorById,
  actors,
  displayNameMap,
  agentState,
  taskById,
  isDark,
  readOnly,
  groupId,
  groupLabelById,
  webModelDeliveryStatus,
  highlightEventId,
  onReply,
  onShowRecipients,
  onCopyLink,
  onCopyContent,
  onRelay,
  onOpenSource,
  onOpenPresentationRef,
  onOpenTaskRef,
  onOpenReplyTarget,
  measureElement,
}: VirtualMessageRowProps) {
  const attachMeasuredRow = useCallback((node: HTMLDivElement | null) => {
    measureElement(node);
  }, [measureElement]);

  return (
    <div
      data-index={virtualRow.index}
      data-message-row="true"
      data-message-id={message.id ? String(message.id) : ""}
      ref={attachMeasuredRow}
      style={{
        position: "absolute",
        top: 0,
        left: 0,
        width: "100%",
        transform: `translateY(${virtualRow.start}px)`,
      }}
      className={compactSpacing ? "pb-3" : "pb-6"}
    >
      <MessageBubble
        event={message}
        actorById={actorById}
        actors={actors}
        displayNameMap={displayNameMap}
        agentState={agentState}
        taskById={taskById}
        isDark={isDark}
        readOnly={readOnly}
        groupId={groupId}
        groupLabelById={groupLabelById}
        webModelDeliveryStatus={webModelDeliveryStatus}
        isHighlighted={!!highlightEventId && String(message.id || "") === String(highlightEventId)}
        collapseHeader={collapseHeader}
        onReply={() => onReply(message)}
        onShowRecipients={() => {
          if (message.id) {
            onShowRecipients(String(message.id));
          }
        }}
        onCopyLink={onCopyLink}
        onCopyContent={onCopyContent}
        onRelay={onRelay}
        onOpenSource={onOpenSource}
        onOpenPresentationRef={onOpenPresentationRef}
        onOpenTaskRef={onOpenTaskRef}
        onOpenReplyTarget={onOpenReplyTarget}
      />
    </div>
  );
});

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
  viewKey: _viewKey,
  initialScrollTargetId,
  initialScrollAnchorId,
  initialScrollAnchorOffsetPx,
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
  forceStickToBottomToken = 0,
  isLoadingHistory = false,
  hasMoreHistory = true,
  onLoadMore,
  resetKey,
}: VirtualMessageListInnerProps) {
  const { t } = useTranslation("chat");
  const parentRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const remeasureRafRef = useRef<number | null>(null);
  const replyJumpClearTimerRef = useRef<number | null>(null);
  const replyJumpNoticeTimerRef = useRef<number | null>(null);
  const [replyJumpHighlightId, setReplyJumpHighlightId] = useState("");
  const [replyJumpNotice, setReplyJumpNotice] = useState("");
  // Message ordering is resolved upstream in useChatTab. The virtual list
  // should render that order verbatim instead of maintaining a second,
  // divergent streaming-order cache locally.
  const displayMessages = messages;
  const shouldVirtualize = shouldUseVirtualizedMessageList(displayMessages.length);
  const topInset = Math.max(0, Number(topInsetPx) || 0);

  const agentStateById = useMemo(() => {
    const m = new Map<string, AgentState>();
    for (const p of agentStates || []) m.set(String(p.id || ""), p);
    return m;
  }, [agentStates]);

  const actorById = useMemo(() => {
    const map = new Map<string, Actor>();
    for (const actor of actors || []) {
      const actorId = String(actor.id || "").trim();
      if (actorId) map.set(actorId, actor);

      const actorTitle = String(actor.title || "").trim();
      if (actorTitle && !map.has(actorTitle)) map.set(actorTitle, actor);

      const actorIdLower = actorId.toLowerCase();
      if (actorIdLower && !map.has(actorIdLower)) map.set(actorIdLower, actor);

      const actorTitleLower = actorTitle.toLowerCase();
      if (actorTitleLower && !map.has(actorTitleLower)) map.set(actorTitleLower, actor);
    }
    return map;
  }, [actors]);

  // Create display name map once at the list level (not per-message)
  const displayNameMap = useActorDisplayNameMap(actors);

  // Stable ref for messages — used by getEstimatedSize to avoid rebuilding
  // the callback (and thus the virtualizer) on every messages change.
  const messagesRef = useRef(displayMessages);
  messagesRef.current = displayMessages;

  const isAtBottomRef = useRef(true);
  const followModeRef = useRef<ChatFollowMode>("follow");
  const prevTailSnapshotRef = useRef(
    getChatTailSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      displayMessages.length,
    )
  );
  const prevTailMutationSnapshotRef = useRef(
    getChatTailMutationSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      "",
    )
  );
  const didInitialScrollRef = useRef(false);
  const scrollRafRef = useRef<number | null>(null);
  const scrollTokenRef = useRef(0);
  const scrollRafScheduledRef = useRef(false);
  const snapshotFlushTimerRef = useRef<number | null>(null);
  // For history loading scroll position preservation (prepend older messages)
  const topLoadArmedRef = useRef(true);
  const pendingPrependCompensationRef = useRef<{
    previousOffset: number;
    previousTotalSize: number;
    anchorId: string;
    anchorOffsetPx: number;
  } | null>(null);
  const lastScrollTopRef = useRef(0);
  // Mark container resize work, such as the footer reply bar appearing or
  // disappearing, so handleScroll does not treat browser-clamped scrollTop as user scroll-up.
  const isContainerResizingRef = useRef(false);
  const forceStickToBottomUntilRef = useRef(0);

  // Track previous resetKey for scroll snapshot before group switch
  const prevResetKeyRef = useRef<string | undefined>(undefined);
  // Store latest scroll snapshot for saving on group switch
  const latestSnapshotRef = useRef<{ mode: ChatFollowMode; anchorId: string; offsetPx: number; updatedAt: number } | null>(null);

  const getEstimatedSize = useCallback(
    (index: number): number => {
      const message = messagesRef.current[index];
      const previousMessage = index > 0 ? messagesRef.current[index - 1] : undefined;
      const grouping = getMessageRowGrouping(previousMessage, message);
      return estimateMessageRowHeight(message, { collapseHeader: grouping.collapseHeader });
    },
    [] // Stable ref — reads from messagesRef.current, no dep on messages array
  );

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: displayMessages.length,
    enabled: shouldVirtualize,
    getScrollElement: () => parentRef.current,
    getItemKey: (index) => getStableMessageKey(displayMessages[index], index),
    estimateSize: getEstimatedSize,
    overscan: 10,
    paddingStart: 72 + topInset,
  });


  // Let tanstack own row measurement via its built-in observer. Layering an
  // extra per-row ResizeObserver on top of measureElement creates duplicate
  // measure -> notify cycles that can recurse during rapid scrolling.
  const measureElement = virtualizer.measureElement;

  const getMessageRowById = useCallback((eventId: string): HTMLDivElement | null => {
    const container = parentRef.current;
    if (!container || !eventId) return null;
    return container.querySelector(`[data-message-row="true"][data-message-id="${CSS.escape(eventId)}"]`);
  }, []);

  const getAnchorSnapshot = useCallback((scrollTop: number) => {
    const container = parentRef.current;
    if (!container) return null;

    if (shouldVirtualize) {
      const vItems = virtualizer.getVirtualItems();
      if (vItems.length <= 0) return null;
      const anchorItem = vItems.find((v) => v.start + v.size > scrollTop + 1) || vItems[0];
      const msg = displayMessages[anchorItem.index];
      const anchorId = msg?.id ? String(msg.id) : "";
      if (!anchorId) return null;
      return {
        anchorId,
        offsetPx: Math.max(0, scrollTop - anchorItem.start),
      };
    }

    const rows = Array.from(container.querySelectorAll<HTMLDivElement>('[data-message-row="true"]'));
    if (rows.length <= 0) return null;
    const anchorRow =
      rows.find((row) => row.offsetTop + row.offsetHeight > scrollTop + 1) || rows[0];
    const anchorId = String(anchorRow.dataset.messageId || "").trim();
    if (!anchorId) return null;
    return {
      anchorId,
      offsetPx: Math.max(0, scrollTop - anchorRow.offsetTop),
    };
  }, [displayMessages, shouldVirtualize, virtualizer]);

  const getCurrentContentSize = useCallback(() => {
    const el = parentRef.current;
    if (!el) return 0;
    return shouldVirtualize ? virtualizer.getTotalSize() : el.scrollHeight;
  }, [shouldVirtualize, virtualizer]);

  const setAtBottom = useCallback((next: boolean) => {
    isAtBottomRef.current = next;
  }, []);

  const setFollowMode = useCallback((next: ChatFollowMode) => {
    followModeRef.current = next;
  }, []);

  const scrollToMessageAnchor = useCallback((eventId: string, offsetPx = 0) => {
    const el = parentRef.current;
    if (!el || !eventId) return false;

    if (shouldVirtualize) {
      const idx = displayMessages.findIndex((m) => String(m?.id || "") === String(eventId));
      if (idx < 0) return false;
      const offsetInfo = virtualizer.getOffsetForIndex(idx, "start");
      if (offsetInfo) {
        virtualizer.scrollToOffset(offsetInfo[0] + Math.max(0, offsetPx), { align: "start", behavior: "auto" });
      } else {
        virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
      }
      return true;
    }

    const row = getMessageRowById(String(eventId));
    if (!row) return false;
    el.scrollTo({ top: row.offsetTop + Math.max(0, offsetPx), behavior: "auto" });
    return true;
  }, [displayMessages, getMessageRowById, shouldVirtualize, virtualizer]);

  const checkIsAtBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el) return true;
    const threshold = 8;
    return el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

  const scrollToBottom = useCallback((opts?: { force?: boolean }) => {
    const el = parentRef.current;
    if (!el || displayMessages.length <= 0) return;
    window.requestAnimationFrame(() => {
      if (!shouldRunScheduledBottomScroll({
        followMode: followModeRef.current,
        isAtBottom: isAtBottomRef.current,
        forceStickToBottom: forceStickToBottomUntilRef.current > performance.now(),
        explicitForce: !!opts?.force,
      })) {
        return;
      }
      el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
    });
  }, [displayMessages.length]);

  const cancelScheduledScroll = useCallback(() => {
    const rid = scrollRafRef.current;
    if (rid != null) {
      scrollRafRef.current = null;
      window.cancelAnimationFrame(rid);
    }
  }, []);

  const shouldForceStickToBottom = useCallback(() => {
    return forceStickToBottomUntilRef.current > performance.now();
  }, []);

  const shouldAutoScrollNow = useCallback(() => {
    if (shouldForceStickToBottom()) return true;
    return shouldAutoScrollToBottom({
      followMode: followModeRef.current,
      isAtBottom: isAtBottomRef.current,
      forceStickToBottom: false,
    });
  }, [shouldForceStickToBottom]);

  const scheduleForceStickToBottom = useCallback(() => {
    forceStickToBottomUntilRef.current = performance.now() + 900;
    cancelScheduledScroll();
    scrollRafRef.current = window.requestAnimationFrame(() => {
      scrollRafRef.current = null;
      if (!shouldForceStickToBottom()) return;
      scrollToBottom({ force: true });
    });
  }, [cancelScheduledScroll, scrollToBottom, shouldForceStickToBottom]);

  const scheduleScroll = useCallback(
    (fn: () => void) => {
      cancelScheduledScroll();
      scrollRafRef.current = window.requestAnimationFrame(() => {
        scrollRafRef.current = null;
        fn();
      });
    },
    [cancelScheduledScroll]
  );

  const scrollToIndexStable = useCallback(
    (idx: number) => {
      cancelScheduledScroll();
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
    [cancelScheduledScroll, virtualizer]
  );

  const scrollToAnchorStable = useCallback(
    (idx: number, offsetPx: number) => {
      cancelScheduledScroll();
      const token = scrollTokenRef.current;
      const doScroll = () => {
        const offsetInfo = virtualizer.getOffsetForIndex(idx, "start");
        if (offsetInfo) {
          virtualizer.scrollToOffset(offsetInfo[0] + Math.max(0, offsetPx), { align: "start", behavior: "auto" });
        } else {
          virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
        }
      };
      doScroll();

      scrollRafRef.current = window.requestAnimationFrame(() => {
        scrollRafRef.current = null;
        if (scrollTokenRef.current !== token) return;
        doScroll();
      });
    },
    [cancelScheduledScroll, virtualizer]
  );

  const showReplyJumpNotice = useCallback((message: string) => {
    if (replyJumpNoticeTimerRef.current != null) {
      window.clearTimeout(replyJumpNoticeTimerRef.current);
      replyJumpNoticeTimerRef.current = null;
    }
    setReplyJumpNotice(message);
    replyJumpNoticeTimerRef.current = window.setTimeout(() => {
      replyJumpNoticeTimerRef.current = null;
      setReplyJumpNotice("");
    }, 2200);
  }, []);

  const handleOpenReplyTarget = useCallback((replyToEventId: string) => {
    const targetId = String(replyToEventId || "").trim();
    if (!targetId) return;

    const idx = displayMessages.findIndex((message) => String(message?.id || "") === targetId);
    if (idx < 0) {
      showReplyJumpNotice(t("replyTargetNotLoaded"));
      return;
    }

    setAtBottom(false);
    setFollowMode("detached");
    forceStickToBottomUntilRef.current = 0;
    cancelScheduledScroll();
    setReplyJumpHighlightId(targetId);
    if (replyJumpClearTimerRef.current != null) {
      window.clearTimeout(replyJumpClearTimerRef.current);
      replyJumpClearTimerRef.current = null;
    }
    replyJumpClearTimerRef.current = window.setTimeout(() => {
      replyJumpClearTimerRef.current = null;
      setReplyJumpHighlightId((current) => (current === targetId ? "" : current));
    }, 2200);

    if (shouldVirtualize) {
      scrollToIndexStable(idx);
      return;
    }

    const el = parentRef.current;
    const row = getMessageRowById(targetId);
    if (!el || !row) {
      showReplyJumpNotice(t("replyTargetNotLoaded"));
      return;
    }
    const top = Math.max(0, row.offsetTop - Math.max(0, (el.clientHeight - row.offsetHeight) / 2));
    el.scrollTo({ top, behavior: "auto" });
    lastScrollTopRef.current = top;
  }, [
    cancelScheduledScroll,
    displayMessages,
    getMessageRowById,
    scrollToIndexStable,
    setAtBottom,
    setFollowMode,
    shouldVirtualize,
    showReplyJumpNotice,
    t,
  ]);

  const handleScroll = useCallback(() => {
    const currentEl = parentRef.current;
    if (currentEl && !isContainerResizingRef.current) {
      const atBottom = checkIsAtBottom();
      const wasAtBottom = isAtBottomRef.current;
      setAtBottom(atBottom);
      if (atBottom) {
        setFollowMode("follow");
      } else {
        setFollowMode("detached");
        forceStickToBottomUntilRef.current = 0;
        cancelScheduledScroll();
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
    if (shouldDetachChatFollowOnScroll({
      followMode: followModeRef.current,
      previousTop,
      currentTop: curTop,
      atBottom,
      isContainerResizing: isContainerResizingRef.current,
      topLoadThresholdPx: topTriggerPx,
    })) {
      setFollowMode("detached");
      forceStickToBottomUntilRef.current = 0;
      cancelScheduledScroll();
    }
    lastScrollTopRef.current = curTop;

    if (atBottom && followModeRef.current === "detached") {
      if (curTop >= previousTop) {
        setFollowMode("follow");
      }
    }
    // Only notify parent when atBottom state actually changes (not on every scroll event)
    // to avoid triggering store updates and re-renders during inertia scrolling.
    const wasAtBottom = isAtBottomRef.current;
    setAtBottom(atBottom);
    if (shouldNotifyScrollChange({ wasAtBottom, atBottom, showScrollButton, chatUnreadCount })) {
      onScrollChange?.(atBottom);
    }

    // Capture a stable "anchor" (first visible message id + offset into that row)
    // so the parent can restore scroll position when switching groups.
    // Save to ref only during scroll; flush to store via debounce (not every frame)
    // to prevent zustand state churn that kills browser scroll inertia.
    const anchor = getAnchorSnapshot(curTop);
    if (anchor) {
      const snap = {
        mode: atBottom ? "follow" as const : followModeRef.current,
        anchorId: atBottom ? "" : anchor.anchorId,
        offsetPx: atBottom ? 0 : anchor.offsetPx,
        updatedAt: Date.now(),
      };
      latestSnapshotRef.current = snap;
      // Debounced flush to store — only after 300ms idle
      if (snapshotFlushTimerRef.current) window.clearTimeout(snapshotFlushTimerRef.current);
      snapshotFlushTimerRef.current = window.setTimeout(() => {
        snapshotFlushTimerRef.current = null;
        if (latestSnapshotRef.current) {
          onScrollSnapshot?.(latestSnapshotRef.current);
        }
      }, 300);
    }

    // Top detection for loading more history.
    //
    // Use a hysteresis "arm/disarm" gate instead of relying on scroll direction.
    // This prevents repeated loads when the scroll position jitters near the top
    // (e.g. due to browser scroll anchoring or dynamic row measurement).
    if (curTop > topRearmPx) topLoadArmedRef.current = true;

    const atTop = curTop < topTriggerPx;
    if (atTop && topLoadArmedRef.current && hasMoreHistory && !isLoadingHistory && onLoadMore) {
      topLoadArmedRef.current = false;
      setFollowMode("detached");
      setAtBottom(false);
      forceStickToBottomUntilRef.current = 0;
      cancelScheduledScroll();
      const anchor = getAnchorSnapshot(curTop);
      pendingPrependCompensationRef.current = {
        previousOffset: curTop,
        previousTotalSize: getCurrentContentSize(),
        anchorId: anchor?.anchorId || "",
        anchorOffsetPx: Number(anchor?.offsetPx || 0),
      };

      onLoadMore();
    }
    });
  }, [cancelScheduledScroll, chatUnreadCount, checkIsAtBottom, getAnchorSnapshot, getCurrentContentSize, hasMoreHistory, isLoadingHistory, onLoadMore, onScrollChange, onScrollSnapshot, setAtBottom, setFollowMode, showScrollButton]);

  // When switching views (group or window-mode), reset internal scroll bookkeeping.
  //
  // Important: this must run before the auto-scroll effects below, otherwise it may
  // cancel their scheduled scrolls (breaking deep-link jump precision).
  useEffect(() => {
    const prevKey = prevResetKeyRef.current;

    // Only reset state when resetKey actually changes (not on re-renders with same key)
    if (prevKey === resetKey) {
      return;
    }

    // Before resetting, save the scroll snapshot from previous group (if any)
    if (prevKey && latestSnapshotRef.current) {
      // Extract groupId from prevKey (format: "groupId:live" or "groupId:window:eventId")
      const prevGroupId = prevKey.split(":")[0];
      if (prevGroupId) {
        // Save the last known scroll position for the previous group
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
    topLoadArmedRef.current = true;
    cancelScheduledScroll();
    if (snapshotFlushTimerRef.current) {
      window.clearTimeout(snapshotFlushTimerRef.current);
      snapshotFlushTimerRef.current = null;
    }
    pendingPrependCompensationRef.current = null;
    lastScrollTopRef.current = 0;
    prevTailSnapshotRef.current = getChatTailSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      displayMessages.length,
    );
    prevTailMutationSnapshotRef.current = getChatTailMutationSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      "",
    );

    // Without key-based remount, the virtualizer keeps stale measurement
    // caches from the previous group. Force a full re-measure so item
    // sizes are recalculated for the new messages.
    if (shouldVirtualize) {
      virtualizer.measure();
    }
  }, [displayMessages, initialScrollAnchorId, initialScrollTargetId, resetKey, cancelScheduledScroll, onScrollSnapshot, setAtBottom, setFollowMode, shouldVirtualize, virtualizer]);

  const tailMutationSignature = useMemo(() => {
    const lastMessage = displayMessages[displayMessages.length - 1];
    if (!lastMessage) return "";
    const data = lastMessage.data && typeof lastMessage.data === "object"
      ? (lastMessage.data as { text?: unknown; attachments?: unknown[]; client_id?: unknown })
      : null;
    const attachmentCount = Array.isArray(data?.attachments) ? data.attachments.length : 0;
    const textLength = typeof data?.text === "string" ? data.text.length : 0;
    const clientId = typeof data?.client_id === "string" ? data.client_id.trim() : "";
    return [
      String(lastMessage.id || "").trim(),
      String(lastMessage.by || "").trim(),
      String(lastMessage.ts || "").trim(),
      clientId,
      textLength,
      attachmentCount,
    ].join("|");
  }, [displayMessages]);

  useEffect(() => {
    if (didInitialScrollRef.current) return;
    if (displayMessages.length <= 0) return;
    didInitialScrollRef.current = true;
    scheduleScroll(() => {
      if (initialScrollTargetId) {
        setAtBottom(false);
        setFollowMode("detached");
        if (shouldVirtualize) {
          const idx = displayMessages.findIndex((m) => String(m?.id || "") === String(initialScrollTargetId));
          if (idx >= 0) {
            scrollToIndexStable(idx);
            return;
          }
        } else if (scrollToMessageAnchor(String(initialScrollTargetId), 0)) {
          return;
        }
      }
      if (initialScrollAnchorId) {
        if (shouldVirtualize) {
          const idx = displayMessages.findIndex((m) => String(m?.id || "") === String(initialScrollAnchorId));
          if (idx >= 0) {
            setAtBottom(false);
            setFollowMode("detached");
            scrollToAnchorStable(idx, Number(initialScrollAnchorOffsetPx || 0));
            return;
          }
        } else if (scrollToMessageAnchor(String(initialScrollAnchorId), Number(initialScrollAnchorOffsetPx || 0))) {
          setAtBottom(false);
          setFollowMode("detached");
          return;
        }
        onScrollSnapshot?.({
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
    displayMessages,
    initialScrollAnchorId,
    initialScrollAnchorOffsetPx,
    initialScrollTargetId,
    scheduleForceStickToBottom,
    scheduleScroll,
    onScrollSnapshot,
    scrollToAnchorStable,
    scrollToBottom,
    scrollToIndexStable,
    scrollToMessageAnchor,
    setAtBottom,
    setFollowMode,
    shouldVirtualize,
  ]);

  useEffect(() => {
    if (!forceStickToBottomToken) return;
    setAtBottom(true);
    setFollowMode("follow");
    scheduleForceStickToBottom();
  }, [forceStickToBottomToken, scheduleForceStickToBottom, setAtBottom, setFollowMode]);

  useEffect(() => {
    const nextTailSnapshot = getChatTailSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      displayMessages.length,
    );
    const nextSnapshot = getChatTailMutationSnapshot(
      displayMessages.length > 0 ? getStableMessageKey(displayMessages[displayMessages.length - 1], displayMessages.length - 1) : null,
      tailMutationSignature,
    );
    const prevTailSnapshot = prevTailSnapshotRef.current;
    const prevSnapshot = prevTailMutationSnapshotRef.current;
    prevTailSnapshotRef.current = nextTailSnapshot;
    prevTailMutationSnapshotRef.current = nextSnapshot;
    if (!didInitialScrollRef.current) return;
    if (isLoadingHistory) return;
    if (!shouldAutoScrollNow()) return;
    if (
      !getAutoFollowTrigger({
        previousTailSnapshot: prevTailSnapshot,
        nextTailSnapshot,
        previousTailMutationSnapshot: prevSnapshot,
        nextTailMutationSnapshot: nextSnapshot,
      })
    ) {
      return;
    }

    scheduleScroll(() => {
      if (!shouldAutoScrollNow()) return;
      scrollToBottom();
    });
  }, [displayMessages, isLoadingHistory, scheduleScroll, scrollToBottom, shouldAutoScrollNow, tailMutationSignature]);

  useEffect(() => cancelScheduledScroll, [cancelScheduledScroll]);

  useEffect(() => {
    return () => {
      if (replyJumpClearTimerRef.current != null) {
        window.clearTimeout(replyJumpClearTimerRef.current);
        replyJumpClearTimerRef.current = null;
      }
      if (replyJumpNoticeTimerRef.current != null) {
        window.clearTimeout(replyJumpNoticeTimerRef.current);
        replyJumpNoticeTimerRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    const scrollEl = parentRef.current;
    const observedEl = contentRef.current;
    if (!scrollEl || !observedEl || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(() => {
      // Observe the message content layer rather than the scroll container.
      // Images, streaming text, and expanded attachment lists change content height
      // without changing the container size; observing only the container misses bottom-follow updates.
      lastScrollTopRef.current = scrollEl.scrollTop;

      if (shouldAutoScrollNow()) {
        scheduleScroll(() => {
          if (!shouldAutoScrollNow()) return;
          scrollToBottom();
        });
      }

      window.requestAnimationFrame(() => {
        lastScrollTopRef.current = scrollEl.scrollTop;
      });
    });
    observer.observe(observedEl);
    return () => observer.disconnect();
  }, [scheduleScroll, scrollToBottom, shouldAutoScrollNow]);

  useEffect(() => {
    return () => {
      if (remeasureRafRef.current != null) {
        window.cancelAnimationFrame(remeasureRafRef.current);
        remeasureRafRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    return () => {
      // Cancel pending debounced flush
      if (snapshotFlushTimerRef.current) {
        window.clearTimeout(snapshotFlushTimerRef.current);
        snapshotFlushTimerRef.current = null;
      }
      // Immediate flush on unmount
      if (latestSnapshotRef.current) {
        const currentGroupId = resetKey.split(":")[0];
        if (currentGroupId) {
          onScrollSnapshot?.(latestSnapshotRef.current, currentGroupId);
        }
      }
      if (scrollRef) {
        scrollRef.current = null;
      }
    };
  }, [onScrollSnapshot, resetKey, scrollRef]);

  useLayoutEffect(() => {
    if (isLoadingHistory) return;
    const pending = pendingPrependCompensationRef.current;
    const el = parentRef.current;
    if (!pending || !el) return;

    pendingPrependCompensationRef.current = null;

    if (pending.anchorId && scrollToMessageAnchor(pending.anchorId, pending.anchorOffsetPx)) {
      lastScrollTopRef.current = el.scrollTop;
      topLoadArmedRef.current = false;
      return;
    }

    const nextTotalSize = getCurrentContentSize();
    const delta = Math.max(0, nextTotalSize - pending.previousTotalSize);
    if (delta <= 0) return;

    const nextTop = pending.previousOffset + delta;
    if (shouldVirtualize) {
      virtualizer.scrollToOffset(nextTop, { align: "start", behavior: "auto" });
    } else {
      el.scrollTo({ top: nextTop, behavior: "auto" });
    }
    lastScrollTopRef.current = nextTop;
    topLoadArmedRef.current = false;
  }, [displayMessages, getCurrentContentSize, isLoadingHistory, scrollToMessageAnchor, shouldVirtualize, virtualizer]);

  const effectiveHighlightEventId = replyJumpHighlightId || highlightEventId;

  return (
    <div className="relative flex-1 min-h-0 flex flex-col">
    <div
      ref={(el) => {
        parentRef.current = el;
        if (scrollRef) scrollRef.current = el;
      }}
      className={classNames("flex-1 min-h-0 overflow-auto px-4 py-4 relative", className)}
      style={{ overflowAnchor: "none" }}
      onScroll={displayMessages.length > 0 ? handleScroll : undefined}
      role="log"
      aria-label="Chat messages"
    >
      {displayMessages.length === 0 ? (
        (isLoadingHistory || hasMoreHistory) ? (
          <div className="flex flex-col items-center justify-center h-full text-center pb-20">
            <div className="glass-panel flex items-center gap-2 rounded-full px-3 py-1.5 text-[var(--color-text-secondary)] shadow-md">
              <div className="animate-spin w-4 h-4 border-2 border-current border-t-transparent rounded-full" />
              <span className="text-xs">{t("loadingHistory", { defaultValue: "Loading..." })}</span>
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center h-full text-center pb-20">
            <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-muted)]">
              <MessageSquareTextIcon size={28} />
            </div>
            <p className="text-sm font-medium text-[var(--color-text-secondary)]">
              {t("emptyStateTitle")}
            </p>
            <p className="text-xs mt-1 text-[var(--color-text-tertiary)]">
              {t("emptyStateSubtitle")}
            </p>
            <div className="mt-4 w-full max-w-sm space-y-2 text-left text-xs text-[var(--color-text-tertiary)]">
              {[
                [t("emptyStateQuickNoteTitle"), t("emptyStateQuickNoteBody")],
                [t("emptyStateAskForemanTitle"), t("emptyStateAskForemanBody")],
                [t("emptyStateDurableTitle"), t("emptyStateDurableBody")],
              ].map(([title, body]) => (
                <div
                  key={title}
                  className="flex gap-2 border-t border-[var(--glass-border-subtle)] pt-2"
                >
                  <span className="text-[var(--color-text-secondary)]">{title}</span>
                  <span>{body}</span>
                </div>
              ))}
            </div>
          </div>
        )
      ) : (
        <>
          {(isLoadingHistory || (!hasMoreHistory && !isLoadingHistory)) && (
            <div
              className="pointer-events-none absolute inset-x-0 z-10 flex justify-center py-3"
              style={{ top: topInset }}
            >
              {isLoadingHistory ? (
                <div className="glass-panel flex items-center gap-2 rounded-full px-3 py-1.5 text-[var(--color-text-secondary)] shadow-md">
                  <div className="animate-spin w-4 h-4 border-2 border-current border-t-transparent rounded-full" />
                  <span className="text-xs">{t("loadingHistory", { defaultValue: "Loading..." })}</span>
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

          {shouldVirtualize ? (
            <div
              ref={contentRef}
              style={{
                height: `${virtualizer.getTotalSize()}px`,
                width: "100%",
                position: "relative",
                contain: "layout paint",
              }}
            >
              {virtualizer.getVirtualItems().map((virtualRow) => {
                const message = displayMessages[virtualRow.index];
                const previousMessage = virtualRow.index > 0 ? displayMessages[virtualRow.index - 1] : undefined;
                const grouping = getMessageRowGrouping(previousMessage, message);
                return (
                  <VirtualMessageRow
                    key={virtualRow.key}
                    virtualRow={virtualRow}
                    message={message}
                    collapseHeader={grouping.collapseHeader}
                    compactSpacing={grouping.compactSpacing}
                    actorById={actorById}
                    actors={actors}
                    displayNameMap={displayNameMap}
                    agentState={agentStateById.get(String(message.by || "")) || null}
                    taskById={taskById}
                    isDark={isDark}
                    readOnly={readOnly}
                    groupId={groupId}
                    groupLabelById={groupLabelById}
                    webModelDeliveryStatus={message.id ? webModelDeliveryStatusByEventId?.[String(message.id)] : undefined}
                    highlightEventId={effectiveHighlightEventId}
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
                );
              })}
            </div>
          ) : (
            <div ref={contentRef} className="w-full" style={{ marginTop: topInset }}>
              {displayMessages.map((message, index) => {
                const previousMessage = index > 0 ? displayMessages[index - 1] : undefined;
                const grouping = getMessageRowGrouping(previousMessage, message);
                return (
                  <div
                    key={String(getStableMessageKey(message, index))}
                    data-message-row="true"
                    data-message-id={message.id ? String(message.id) : ""}
                    className={grouping.compactSpacing ? "pb-3" : "pb-6"}
                  >
                    <MessageBubble
                      event={message}
                      actorById={actorById}
                      actors={actors}
                      displayNameMap={displayNameMap}
                      agentState={agentStateById.get(String(message.by || "")) || null}
                      taskById={taskById}
                      isDark={isDark}
                      readOnly={readOnly}
                      groupId={groupId}
                      groupLabelById={groupLabelById}
                      webModelDeliveryStatus={message.id ? webModelDeliveryStatusByEventId?.[String(message.id)] : undefined}
                      isHighlighted={!!effectiveHighlightEventId && String(message.id || "") === String(effectiveHighlightEventId)}
                      collapseHeader={grouping.collapseHeader}
                      onReply={() => onReply(message)}
                      onShowRecipients={() => {
                        if (message.id) {
                          onShowRecipients(String(message.id));
                        }
                      }}
                      onCopyLink={onCopyLink}
                      onCopyContent={onCopyContent}
                      onRelay={onRelay}
                      onOpenSource={onOpenSource}
                      onOpenPresentationRef={onOpenPresentationRef}
                      onOpenTaskRef={onOpenTaskRef}
                      onOpenReplyTarget={handleOpenReplyTarget}
                    />
                  </div>
                );
              })}
            </div>
          )}
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
