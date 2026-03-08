import { memo, useRef, useEffect, useCallback, useMemo } from "react";
import type { MutableRefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { LedgerEvent, Actor, AgentState } from "../types";
import { MessageBubble } from "./MessageBubble";
import { useActorDisplayNameMap } from "../hooks/useActorDisplayName";

export interface VirtualMessageListProps {
  messages: LedgerEvent[];
  actors: Actor[];
  agentStates: AgentState[];
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  viewKey?: string;
  initialScrollTargetId?: string;
  initialScrollAnchorId?: string;
  initialScrollAnchorOffsetPx?: number;
  highlightEventId?: string;
  scrollRef?: MutableRefObject<HTMLDivElement | null>;
  onReply: (ev: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  onRelay?: (ev: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  showScrollButton: boolean;
  onScrollButtonClick: () => void;
  chatUnreadCount: number;
  onScrollChange?: (isAtBottom: boolean) => void;
  onScrollSnapshot?: (snap: { atBottom: boolean; anchorId: string; offsetPx: number }, groupId?: string) => void;
  // History loading
  isLoadingHistory?: boolean;
  hasMoreHistory?: boolean;
  onLoadMore?: () => void;
}

export const VirtualMessageList = memo(function VirtualMessageList({
  messages,
  actors,
  agentStates,
  isDark,
  readOnly,
  groupId,
  groupLabelById,
  viewKey,
  initialScrollTargetId,
  initialScrollAnchorId,
  initialScrollAnchorOffsetPx,
  highlightEventId,
  scrollRef,
  onReply,
  onShowRecipients,
  onCopyLink,
  onRelay,
  onOpenSource,
  showScrollButton,
  onScrollButtonClick,
  chatUnreadCount,
  onScrollChange,
  onScrollSnapshot,
  isLoadingHistory = false,
  hasMoreHistory = true,
  onLoadMore,
}: VirtualMessageListProps) {
  const parentRef = useRef<HTMLDivElement | null>(null);

  const agentStateById = useMemo(() => {
    const m = new Map<string, AgentState>();
    for (const p of agentStates || []) m.set(String(p.id || ""), p);
    return m;
  }, [agentStates]);

  // Create display name map once at the list level (not per-message)
  const displayNameMap = useActorDisplayNameMap(actors);

  const prevMessageCountRef = useRef(messages.length);
  const isAtBottomRef = useRef(true);
  const didInitialScrollRef = useRef(false);
  const scrollTimeoutRef = useRef<number | null>(null);
  const scrollRafRef = useRef<number | null>(null);
  const followupScrollTimeoutRef = useRef<number | null>(null);
  const scrollTokenRef = useRef(0);
  const scrollRafScheduledRef = useRef(false);
  const resetKey = viewKey ?? groupId;

  // For history loading scroll position preservation (prepend older messages)
  const topLoadArmedRef = useRef(true);
  const pendingRestoreRef = useRef(false);
  const anchorMessageIdRef = useRef<string>("");
  const anchorOffsetRef = useRef(0);

  // Track previous resetKey for scroll snapshot before group switch
  const prevResetKeyRef = useRef<string | undefined>(undefined);
  // Store latest scroll snapshot for saving on group switch
  const latestSnapshotRef = useRef<{ atBottom: boolean; anchorId: string; offsetPx: number } | null>(null);

  // Dynamic height estimation based on message content
  // This reduces layout shift during scrolling by providing better initial estimates
  const getEstimatedSize = useCallback(
    (index: number): number => {
      const message = messages[index];
      if (!message) return 100;

      const data = message.data as { text?: string; attachments?: unknown[] } | undefined;
      const text = String(data?.text || "");
      const attachments = Array.isArray(data?.attachments) ? data.attachments : [];

      // Base height: avatar + header + padding
      let height = 72;

      // Text content estimation
      if (text) {
        const lineCount = text.split("\n").length;
        // Average ~20 chars per line on mobile, ~60 on desktop
        const avgCharsPerLine = 40;
        const wrapLines = Math.ceil(text.length / avgCharsPerLine);
        const estimatedLines = Math.max(lineCount, wrapLines);

        // ~20px per line of text, with a minimum for short messages
        height += Math.min(estimatedLines * 20, 400); // Cap at 400px for very long messages

        // Code blocks detection (triple backticks)
        const codeBlockCount = (text.match(/```/g) || []).length / 2;
        if (codeBlockCount > 0) {
          // Code blocks add significant height due to monospace font and padding
          height += codeBlockCount * 80;
        }
      }

      // Attachments add height
      if (attachments.length > 0) {
        // Images are ~200px, files are ~48px
        for (const att of attachments) {
          const mime = String((att as { mime_type?: string })?.mime_type || "");
          if (mime.startsWith("image/")) {
            height += 200;
          } else {
            height += 48;
          }
        }
      }

      // Quoted reply adds ~60px
      const quoteText = (data as { quote_text?: string })?.quote_text;
      if (quoteText) {
        height += 60;
      }

      return height;
    },
    [messages]
  );

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    getItemKey: (index) => messages[index]?.id ?? index,
    estimateSize: getEstimatedSize,
    overscan: 5,
    paddingStart: 72,
  });


  const measureElement = useCallback(
    (node: HTMLDivElement | null) => {
      if (!node) return;
      if (typeof queueMicrotask === "function") {
        queueMicrotask(() => virtualizer.measureElement(node));
      } else {
        Promise.resolve().then(() => virtualizer.measureElement(node));
      }
    },
    [virtualizer]
  );

  const checkIsAtBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el) return true;
    // Increased threshold to reduce false negatives when near bottom
    // (e.g., during layout shifts or when switching groups)
    const threshold = 200;
    return el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

  const scrollToBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el || messages.length <= 0) return;
    requestAnimationFrame(() => {
      el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
    });
  }, [messages.length]);

  const cancelScheduledScroll = useCallback(() => {
    const id = scrollTimeoutRef.current;
    if (id != null) {
      scrollTimeoutRef.current = null;
      window.clearTimeout(id);
    }
    const rid = scrollRafRef.current;
    if (rid != null) {
      scrollRafRef.current = null;
      window.cancelAnimationFrame(rid);
    }
    const fid = followupScrollTimeoutRef.current;
    if (fid != null) {
      followupScrollTimeoutRef.current = null;
      window.clearTimeout(fid);
    }
  }, []);

  const scheduleScroll = useCallback(
    (fn: () => void) => {
      cancelScheduledScroll();
      // Use setTimeout(0) to avoid flushSync warning during React lifecycle.
      scrollTimeoutRef.current = window.setTimeout(() => {
        scrollTimeoutRef.current = null;
        fn();
      }, 0);
    },
    [cancelScheduledScroll]
  );

  const scheduleScrollToBottom = useCallback(
    (opts?: { requireAtBottom?: boolean }) => {
      scheduleScroll(() => {
        if (opts?.requireAtBottom && !isAtBottomRef.current) return;
        scrollToBottom();
      });
    },
    [scheduleScroll, scrollToBottom]
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

        followupScrollTimeoutRef.current = window.setTimeout(() => {
          followupScrollTimeoutRef.current = null;
          if (scrollTokenRef.current !== token) return;
          doScroll();
        }, 120);
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

        followupScrollTimeoutRef.current = window.setTimeout(() => {
          followupScrollTimeoutRef.current = null;
          if (scrollTokenRef.current !== token) return;
          doScroll();
        }, 120);
      });
    },
    [cancelScheduledScroll, virtualizer]
  );

  const handleScroll = useCallback(() => {
    if (scrollRafScheduledRef.current) return;
    scrollRafScheduledRef.current = true;

    window.requestAnimationFrame(() => {
      scrollRafScheduledRef.current = false;

    const el = parentRef.current;
    if (!el) return;

    const curTop = el.scrollTop;

    const atBottom = checkIsAtBottom();
    isAtBottomRef.current = atBottom;
    onScrollChange?.(atBottom);

    // Capture a stable "anchor" (first visible message id + offset into that row)
    // so the parent can restore scroll position when switching groups.
    const vItems = virtualizer.getVirtualItems();
    if (vItems.length > 0) {
      const anchorItem =
        vItems.find((v) => v.start + v.size > curTop + 1) || vItems[0];
      const msg = messages[anchorItem.index];
      const anchorId = msg?.id ? String(msg.id) : "";
      if (anchorId) {
        const offsetPx = Math.max(0, curTop - anchorItem.start);
        const snap = { atBottom, anchorId, offsetPx };
        latestSnapshotRef.current = snap;
        onScrollSnapshot?.(snap);
      }
    }

    // Top detection for loading more history.
    //
    // Use a hysteresis "arm/disarm" gate instead of relying on scroll direction.
    // This prevents repeated loads when the scroll position jitters near the top
    // (e.g. due to browser scroll anchoring or dynamic row measurement).
    const topTriggerPx = 80;
    const topRearmPx = 240;
    if (curTop > topRearmPx) topLoadArmedRef.current = true;

    const atTop = curTop < topTriggerPx;
    if (atTop && topLoadArmedRef.current && hasMoreHistory && !isLoadingHistory && onLoadMore) {
      topLoadArmedRef.current = false;
      pendingRestoreRef.current = true;

      const first = vItems[0];
      const firstMsg = first ? messages[first.index] : null;
      anchorMessageIdRef.current = firstMsg?.id ? String(firstMsg.id) : "";
      anchorOffsetRef.current = first ? Math.max(0, curTop - first.start) : 0;

      onLoadMore();
    }
    });
  }, [checkIsAtBottom, hasMoreHistory, isLoadingHistory, messages, onLoadMore, onScrollChange, onScrollSnapshot, virtualizer]);

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
    prevMessageCountRef.current = 0;
    isAtBottomRef.current = true;
    didInitialScrollRef.current = false;
    topLoadArmedRef.current = true;
    cancelScheduledScroll();
    pendingRestoreRef.current = false;
    anchorMessageIdRef.current = "";
    anchorOffsetRef.current = 0;
  }, [resetKey, cancelScheduledScroll, onScrollSnapshot]);

  useEffect(() => {
    const prevCount = prevMessageCountRef.current;
    const newCount = messages.length;
    prevMessageCountRef.current = newCount;

    if (newCount > prevCount && isAtBottomRef.current) {
      scheduleScrollToBottom({ requireAtBottom: true });
    }
  }, [messages.length, scheduleScrollToBottom]);

  useEffect(() => {
    if (didInitialScrollRef.current) return;
    if (messages.length <= 0) return;
    didInitialScrollRef.current = true;
    scheduleScroll(() => {
      if (initialScrollTargetId) {
        isAtBottomRef.current = false;
        const idx = messages.findIndex((m) => String(m?.id || "") === String(initialScrollTargetId));
        if (idx >= 0) {
          scrollToIndexStable(idx);
          return;
        }
      }
      if (initialScrollAnchorId) {
        const idx = messages.findIndex((m) => String(m?.id || "") === String(initialScrollAnchorId));
        if (idx >= 0) {
          isAtBottomRef.current = false;
          scrollToAnchorStable(idx, Number(initialScrollAnchorOffsetPx || 0));
          return;
        }
      }
      scrollToBottom();
    });
  }, [initialScrollAnchorId, initialScrollAnchorOffsetPx, initialScrollTargetId, messages, scheduleScroll, scrollToAnchorStable, scrollToBottom, scrollToIndexStable]);

  useEffect(() => cancelScheduledScroll, [cancelScheduledScroll]);

  // Restore scroll position after loading older messages
  useEffect(() => {
    if (isLoadingHistory) return;
    if (!pendingRestoreRef.current) return;
    const el = parentRef.current;
    if (!el) return;

    pendingRestoreRef.current = false;

    const anchorId = anchorMessageIdRef.current;
    const offsetInRow = anchorOffsetRef.current;
    anchorMessageIdRef.current = "";
    anchorOffsetRef.current = 0;

    if (anchorId) {
      const idx = messages.findIndex((m) => String(m.id || "") === anchorId);
      if (idx >= 0) {
        const offsetInfo = virtualizer.getOffsetForIndex(idx, "start");
        if (offsetInfo) {
          virtualizer.scrollToOffset(offsetInfo[0] + offsetInRow, { align: "start", behavior: "auto" });
        } else {
          virtualizer.scrollToIndex(idx, { align: "start", behavior: "auto" });
        }
      }
    }
  }, [isLoadingHistory, messages, virtualizer]);

  return (
    <div
      ref={(el) => {
        parentRef.current = el;
        if (scrollRef) scrollRef.current = el;
      }}
      className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
      style={{ overflowAnchor: "none" }}
      onScroll={messages.length > 0 ? handleScroll : undefined}
      role="log"
      aria-label="Chat messages"
    >
      {messages.length === 0 ? (
        isLoadingHistory ? (
          <div className="flex flex-col items-center justify-center h-full text-center pb-20">
            <div
              className={`flex items-center gap-2 px-3 py-1.5 rounded-full shadow-md ${isDark ? "bg-slate-800 text-slate-300" : "bg-white text-gray-600"
                }`}
            >
              <div className="animate-spin w-4 h-4 border-2 border-current border-t-transparent rounded-full" />
              <span className="text-xs">Loading...</span>
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center h-full text-center pb-20 opacity-50">
            <div className="text-4xl mb-4 grayscale">💬</div>
            <p className={`text-sm font-medium ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              No messages yet
            </p>
            <p className={`text-xs mt-1 ${isDark ? "text-slate-600" : "text-gray-400"}`}>
              Start the conversation with your AI team.
            </p>
          </div>
        )
      ) : (
        <>
          {/* Loading indicator for history - sticky positioned for visibility */}
          {isLoadingHistory && (
            <div className="sticky top-0 z-10 flex justify-center py-3">
              <div className={`flex items-center gap-2 px-3 py-1.5 rounded-full shadow-md ${isDark ? "bg-slate-800 text-slate-300" : "bg-white text-gray-600"
                }`}>
                <div className="animate-spin w-4 h-4 border-2 border-current border-t-transparent rounded-full" />
                <span className="text-xs">Loading...</span>
              </div>
            </div>
          )}

          {/* No more history indicator */}
          {!hasMoreHistory && !isLoadingHistory && (
            <div className={`text-center py-4 text-sm ${isDark ? "text-slate-500" : "text-gray-400"}`}>
              No more messages
            </div>
          )}

          {/* Virtual list container */}
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: "100%",
              position: "relative",
              contain: "strict",
            }}
          >
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const message = messages[virtualRow.index];
              return (
                <div
                  key={virtualRow.key}
                  data-index={virtualRow.index}
                  ref={measureElement}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                  className="pb-6"
                >
                  <MessageBubble
                    event={message}
                    actors={actors}
                    displayNameMap={displayNameMap}
                    agentState={agentStateById.get(String(message.by || "")) || null}
                    isDark={isDark}
                    readOnly={readOnly}
                    groupId={groupId}
                    groupLabelById={groupLabelById}
                    isHighlighted={!!highlightEventId && String(message.id || "") === String(highlightEventId)}
                    onReply={() => onReply(message)}
                    onShowRecipients={() => {
                      if (message.id) {
                        onShowRecipients(String(message.id));
                      }
                    }}
                    onCopyLink={onCopyLink}
                    onRelay={onRelay}
                    onOpenSource={onOpenSource}
                  />
                </div>
              );
            })}
          </div>

          {/* Scroll Button */}
          {!readOnly && showScrollButton && (
            <button
              className={`fixed bottom-36 right-6 p-3 rounded-full shadow-xl transition-all z-10 ${isDark
                ? "bg-slate-800 text-white hover:bg-slate-700 border border-slate-700"
                : "bg-white text-gray-600 hover:bg-gray-50 border border-gray-100"
                }`}
              onClick={() => {
                scrollToBottom();
                onScrollButtonClick();
              }}
              aria-label="Scroll to bottom"
            >
              <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 14l-7 7m0 0l-7-7m7 7V3" />
              </svg>
              {chatUnreadCount > 0 && (
                <span className="absolute -top-1 -right-1 flex h-4 w-4">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-indigo-400 opacity-60"></span>
                  <span className="relative inline-flex rounded-full h-4 w-4 bg-indigo-500 text-[9px] text-white items-center justify-center font-bold">
                    {chatUnreadCount > 9 ? "!" : chatUnreadCount}
                  </span>
                </span>
              )}
            </button>
          )}
        </>
      )}
    </div>
  );
});
