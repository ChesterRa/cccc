import { useEffect, useMemo } from "react";
import type { MutableRefObject } from "react";
import type { LedgerEvent } from "../../types";
import { getChatTailMutationSnapshot, getChatTailSnapshot } from "../../utils/chatAutoFollow";
import { getAutoFollowTrigger, getStableMessageKey } from "../virtualMessageListHelpers";

type MessageTailAutoFollowOptions = {
  messages: LedgerEvent[];
  didInitialScrollRef: MutableRefObject<boolean>;
  previousTailRef: MutableRefObject<ReturnType<typeof getChatTailSnapshot>>;
  previousMutationRef: MutableRefObject<ReturnType<typeof getChatTailMutationSnapshot>>;
  previousContentSizeRef: MutableRefObject<number>;
  getCurrentContentSize: () => number;
  isLoadingHistory: boolean;
  shouldAutoScroll: (options: { previousContentSize?: number }) => boolean;
  scheduleScroll: (action: () => void) => void;
  scrollToBottom: () => void;
};

export function useMessageTailAutoFollow({
  messages,
  didInitialScrollRef,
  previousTailRef,
  previousMutationRef,
  previousContentSizeRef,
  getCurrentContentSize,
  isLoadingHistory,
  shouldAutoScroll,
  scheduleScroll,
  scrollToBottom,
}: MessageTailAutoFollowOptions) {
  const mutationSignature = useMemo(() => {
    const lastMessage = messages[messages.length - 1];
    if (!lastMessage) return "";
    const data =
      lastMessage.data && typeof lastMessage.data === "object"
        ? (lastMessage.data as {
            text?: unknown;
            insight?: unknown;
            attachments?: unknown[];
            client_id?: unknown;
          })
        : null;
    return [
      String(lastMessage.id || "").trim(),
      String(lastMessage.by || "").trim(),
      String(lastMessage.ts || "").trim(),
      typeof data?.client_id === "string" ? data.client_id.trim() : "",
      typeof data?.text === "string" ? data.text.length : 0,
      typeof data?.insight === "string" ? data.insight.length : 0,
      Array.isArray(data?.attachments) ? data.attachments.length : 0,
    ].join("|");
  }, [messages]);

  useEffect(() => {
    const tailKey =
      messages.length > 0
        ? getStableMessageKey(messages[messages.length - 1], messages.length - 1)
        : null;
    const nextTail = getChatTailSnapshot(tailKey, messages.length);
    const nextMutation = getChatTailMutationSnapshot(tailKey, mutationSignature);
    const previousTail = previousTailRef.current;
    const previousMutation = previousMutationRef.current;
    const previousContentSize = previousContentSizeRef.current;
    previousTailRef.current = nextTail;
    previousMutationRef.current = nextMutation;
    previousContentSizeRef.current = getCurrentContentSize();
    if (
      !didInitialScrollRef.current ||
      isLoadingHistory ||
      !shouldAutoScroll({ previousContentSize })
    )
      return;
    if (
      !getAutoFollowTrigger({
        previousTailSnapshot: previousTail,
        nextTailSnapshot: nextTail,
        previousTailMutationSnapshot: previousMutation,
        nextTailMutationSnapshot: nextMutation,
      })
    ) {
      return;
    }
    scheduleScroll(() => {
      if (shouldAutoScroll({ previousContentSize })) scrollToBottom();
    });
  }, [
    didInitialScrollRef,
    getCurrentContentSize,
    isLoadingHistory,
    messages,
    mutationSignature,
    previousContentSizeRef,
    previousMutationRef,
    previousTailRef,
    scheduleScroll,
    scrollToBottom,
    shouldAutoScroll,
  ]);
}
