import { useCallback, type KeyboardEvent, type MutableRefObject } from "react";

import type { ScrollToBottom } from "./useBottomScrollController";

const KEYBOARD_SCROLL_KEYS = new Set([
  "ArrowDown",
  "ArrowUp",
  "End",
  "Home",
  "PageDown",
  "PageUp",
  " ",
  "Spacebar",
]);

export function useForcedBottomFollowKeyboardCancel(cancel: () => void) {
  return useCallback(
    (event: KeyboardEvent<HTMLElement>) => {
      if (KEYBOARD_SCROLL_KEYS.has(event.key)) cancel();
    },
    [cancel],
  );
}

export function useForcedBottomFollow(args: {
  requestTokenRef: MutableRefObject<number>;
  scrollRafRef: MutableRefObject<number | null>;
  forceStickToBottomRef: MutableRefObject<boolean>;
  scrollToBottom: ScrollToBottom;
}) {
  const { requestTokenRef, scrollRafRef, forceStickToBottomRef, scrollToBottom } = args;
  const cancelScheduledScroll = useCallback(() => {
    const requestId = scrollRafRef.current;
    if (requestId == null) return;
    scrollRafRef.current = null;
    window.cancelAnimationFrame(requestId);
  }, [scrollRafRef]);

  const cancelPendingBottomScroll = useCallback(() => {
    requestTokenRef.current += 1;
    forceStickToBottomRef.current = false;
    cancelScheduledScroll();
  }, [cancelScheduledScroll, forceStickToBottomRef, requestTokenRef]);

  const shouldForceStickToBottom = useCallback(
    () => forceStickToBottomRef.current,
    [forceStickToBottomRef],
  );

  const scheduleForceStickToBottom = useCallback(() => {
    requestTokenRef.current += 1;
    const requestToken = requestTokenRef.current;
    forceStickToBottomRef.current = true;
    cancelScheduledScroll();
    scrollRafRef.current = window.requestAnimationFrame(() => {
      scrollRafRef.current = null;
      if (requestTokenRef.current !== requestToken) return;
      scrollToBottom({ force: true, requestToken });
    });
  }, [cancelScheduledScroll, forceStickToBottomRef, requestTokenRef, scrollRafRef, scrollToBottom]);

  return {
    cancelPendingBottomScroll,
    cancelScheduledScroll,
    scheduleForceStickToBottom,
    shouldForceStickToBottom,
  };
}
