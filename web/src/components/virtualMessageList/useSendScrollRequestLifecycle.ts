import { useEffect } from "react";
import {
  isChatSendScrollRequestOwner,
  type ChatSendScrollRequest,
} from "../../utils/chatSendScrollRequest";
import type { ChatFollowMode } from "../../stores/useUIStore";
import type { MutableRefObject } from "react";

type ScrollRequestRef = { current: number };
type ScrollToBottom = (options?: {
  force?: boolean;
  requestToken?: number;
  behavior?: ScrollBehavior;
  sendRequest?: ChatSendScrollRequest;
}) => void;

export function useSendScrollRequestLifecycle(args: {
  groupId: string;
  viewKey: string;
  request?: ChatSendScrollRequest | null;
  activeRequestRef: MutableRefObject<ChatSendScrollRequest | null>;
  onConsumed?: (requestId: number) => void;
  setAtBottom: (value: boolean) => void;
  setFollowMode: (value: ChatFollowMode) => void;
  requestTokenRef: ScrollRequestRef;
  scheduleScroll: (callback: () => void) => void;
  scrollToBottom: ScrollToBottom;
  cancelPendingBottomScroll: () => void;
}) {
  const activeRequestRef = args.activeRequestRef;
  const cancelPendingBottomScroll = args.cancelPendingBottomScroll;

  useEffect(() => {
    const request = args.request;
    if (!request) return;
    if (!isChatSendScrollRequestOwner(request, args.groupId, args.viewKey)) {
      args.onConsumed?.(request.requestId);
      return;
    }
    if (activeRequestRef.current?.requestId === request.requestId) return;
    activeRequestRef.current = request;
    args.onConsumed?.(request.requestId);
    args.setAtBottom(true);
    args.setFollowMode("follow");
    args.requestTokenRef.current += 1;
    const requestToken = args.requestTokenRef.current;
    args.scheduleScroll(() =>
      args.scrollToBottom({ force: true, requestToken, behavior: "smooth", sendRequest: request }),
    );
  }, [activeRequestRef, args]);

  useEffect(
    () => () => {
      activeRequestRef.current = null;
      cancelPendingBottomScroll();
    },
    [activeRequestRef, cancelPendingBottomScroll],
  );

  return activeRequestRef;
}
