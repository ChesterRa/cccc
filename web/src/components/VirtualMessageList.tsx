import { memo, useRef, useEffect, useCallback } from "react";
import type { MutableRefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { LedgerEvent, Actor } from "../types";
import { MessageBubble } from "./MessageBubble";

export interface VirtualMessageListProps {
    messages: LedgerEvent[];
    actors: Actor[];
    isDark: boolean;
    groupId: string;
    scrollRef?: MutableRefObject<HTMLDivElement | null>;
    onReply: (ev: LedgerEvent) => void;
    onShowRecipients: (eventId: string) => void;
    showScrollButton: boolean;
    onScrollButtonClick: () => void;
    chatUnreadCount: number;
    onScrollChange?: (isAtBottom: boolean) => void;
    initialScrollTop?: number; // 用于恢复滚动位置
}

export const VirtualMessageList = memo(function VirtualMessageList({
    messages,
    actors,
    isDark,
    groupId,
    scrollRef,
    onReply,
    onShowRecipients,
    showScrollButton,
    onScrollButtonClick,
    chatUnreadCount,
    onScrollChange,
    initialScrollTop,
}: VirtualMessageListProps) {
  const parentRef = useRef<HTMLDivElement | null>(null);

  const prevMessageCountRef = useRef(0); // 初始为 0，确保首次有消息时能触发滚动
  const isAtBottomRef = useRef(true);
  const didInitialScrollRef = useRef(false);
  const scrollTimeoutRef = useRef<number | null>(null);

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 120,
    overscan: 5,
    // initialOffset is only used when there is a saved scroll position
    ...(initialScrollTop && initialScrollTop > 0
      ? { initialOffset: initialScrollTop }
      : {}),
  });

  // Wrap measureElement to avoid flushSync warning during React render
  const measureElement = useCallback(
    (node: HTMLDivElement | null) => {
      if (node) {
        // Defer measurement to next microtask to avoid flushSync during render
        queueMicrotask(() => virtualizer.measureElement(node));
      }
    },
    [virtualizer]
  );

  // Check if scrolled to bottom
  const checkIsAtBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el) return true;
    const threshold = 100;
    return el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
  }, []);

  // Scroll to bottom - Use native scrolling to avoid boundary issues with virtualizer.scrollToIndex
  const scrollToBottom = useCallback(() => {
    const el = parentRef.current;
    if (!el || messages.length <= 0) return;
    // use requestAnimationFrame
    requestAnimationFrame(() => {
      el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
    });
  }, [messages.length]);

  const cancelScheduledScroll = useCallback(() => {
    const id = scrollTimeoutRef.current;
    if (id == null) return;
    scrollTimeoutRef.current = null;
    window.clearTimeout(id);
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

  const scheduleScrollToBottom = useCallback(() => {
    scheduleScroll(() => scrollToBottom());
  }, [scheduleScroll, scrollToBottom]);

  // Handle scroll events
  const handleScroll = useCallback(() => {
    const atBottom = checkIsAtBottom();
    isAtBottomRef.current = atBottom;
    onScrollChange?.(atBottom);
  }, [checkIsAtBottom, onScrollChange]);

  // Auto-scroll when new messages arrive and user was at bottom
  useEffect(() => {
    const prevCount = prevMessageCountRef.current;
    const newCount = messages.length;
    prevMessageCountRef.current = newCount;

    // 在此刻捕获 isAtBottom 状态，不要在异步回调中再检查
    // 因为 virtualizer 重新渲染可能导致短暂的滚动事件改变 isAtBottomRef
    if (newCount > prevCount && isAtBottomRef.current) {
      scheduleScrollToBottom();
    }
  }, [messages.length, scheduleScrollToBottom]);

  // Initial scroll to bottom (only when no initialOffset is provided)
  useEffect(() => {
    if (didInitialScrollRef.current) return;
    if (messages.length <= 0) return;
    didInitialScrollRef.current = true;
    // 如果有 initialScrollTop，virtualizer 的 initialOffset 已处理
    // 否则滚动到底部
    if (!initialScrollTop || initialScrollTop <= 0) {
      scheduleScrollToBottom();
    } else {
      // 更新 isAtBottom 状态
      isAtBottomRef.current = checkIsAtBottom();
    }
  }, [
    messages.length,
    scheduleScrollToBottom,
    initialScrollTop,
    checkIsAtBottom,
  ]);

  // When switching groups, default to showing the latest messages.
  // Important: do NOT depend on messages.length here, otherwise every new message would
  // look like a "group switch" and force-scroll to bottom even if the user scrolled up.
  useEffect(() => {
    prevMessageCountRef.current = 0;
    isAtBottomRef.current = true;
    didInitialScrollRef.current = false;
    cancelScheduledScroll();
  }, [groupId, cancelScheduledScroll]);

  // Cleanup scheduled scrolls.
  useEffect(() => cancelScheduledScroll, [cancelScheduledScroll]);

  return (
    <div
      ref={(el) => {
        parentRef.current = el;
        if (scrollRef) scrollRef.current = el;
      }}
      className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
      onScroll={messages.length > 0 ? handleScroll : undefined}
      role="log"
      aria-label="Chat messages"
    >
      {messages.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-full text-center pb-20 opacity-50">
          <div className="text-4xl mb-4 grayscale">💬</div>
          <p
            className={`text-sm font-medium ${
              isDark ? "text-slate-400" : "text-gray-500"
            }`}
          >
            No messages yet
          </p>
          <p
            className={`text-xs mt-1 ${
              isDark ? "text-slate-600" : "text-gray-400"
            }`}
          >
            Start the conversation with your AI team.
          </p>
        </div>
      ) : (
        <>
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
                  key={message.id || virtualRow.index}
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
                    isDark={isDark}
                    groupId={groupId}
                    onReply={() => onReply(message)}
                    onShowRecipients={() => {
                      if (message.id) {
                        onShowRecipients(String(message.id));
                      }
                    }}
                  />
                </div>
              );
            })}
          </div>

          {/* Scroll Button */}
          {showScrollButton && (
            <button
              className={`fixed bottom-24 right-6 p-3 rounded-full shadow-xl transition-all z-10 ${
                isDark
                  ? "bg-slate-800 text-white hover:bg-slate-700 border border-slate-700"
                  : "bg-white text-gray-600 hover:bg-gray-50 border border-gray-100"
              }`}
              onClick={() => {
                scrollToBottom();
                onScrollButtonClick();
              }}
              aria-label="Scroll to bottom"
            >
              <svg
                className="w-5 h-5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M19 14l-7 7m0 0l-7-7m7 7V3"
                />
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
