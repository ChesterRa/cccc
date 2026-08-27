import { useCallback, useRef, type MutableRefObject } from "react";

import type { ChatFollowMode } from "../../stores/useUIStore";
import {
  shouldExecuteChatSendScroll,
  type ChatSendScrollRequest,
} from "../../utils/chatSendScrollRequest";
import { shouldRunScheduledBottomScroll } from "../virtualMessageListHelpers";

export type ScrollToBottomOptions = {
  force?: boolean;
  requestToken?: number;
  behavior?: ScrollBehavior;
  sendRequest?: ChatSendScrollRequest;
};
export type ScrollToBottom = (options?: ScrollToBottomOptions) => void;

export function useBottomScrollController(args: {
  parentRef: MutableRefObject<HTMLDivElement | null>;
  messageCount: number;
  groupId: string;
  viewKey: string;
  requestTokenRef: MutableRefObject<number>;
  followModeRef: MutableRefObject<ChatFollowMode>;
  isAtBottomRef: MutableRefObject<boolean>;
  forceStickToBottomRef: MutableRefObject<boolean>;
}) {
  const activeRequestRef = useRef<ChatSendScrollRequest | null>(null);
  const scrollToBottom = useCallback(
    (options?: ScrollToBottomOptions) => {
      const element = args.parentRef.current;
      if (!element || args.messageCount <= 0) return;
      window.requestAnimationFrame(() => {
        if (
          options?.sendRequest &&
          !shouldExecuteChatSendScroll({
            request: options.sendRequest,
            activeRequest: activeRequestRef.current,
            requestToken: options.requestToken ?? -1,
            currentRequestToken: args.requestTokenRef.current,
            groupId: args.groupId,
            viewKey: args.viewKey,
          })
        ) {
          return;
        }
        if (
          !options?.sendRequest &&
          options?.requestToken != null &&
          args.requestTokenRef.current !== options.requestToken
        ) {
          return;
        }
        if (
          !shouldRunScheduledBottomScroll({
            followMode: args.followModeRef.current,
            isAtBottom: args.isAtBottomRef.current,
            forceStickToBottom: args.forceStickToBottomRef.current,
            explicitForce: !!options?.force,
          })
        ) {
          return;
        }
        element.scrollTo({ top: element.scrollHeight, behavior: options?.behavior ?? "auto" });
        if (options?.sendRequest?.requestId === activeRequestRef.current?.requestId) {
          activeRequestRef.current = null;
        }
      });
    },
    [args],
  );

  return { activeRequestRef, scrollToBottom };
}
