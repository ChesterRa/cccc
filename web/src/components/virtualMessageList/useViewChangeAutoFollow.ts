import { useEffect, useRef } from "react";

import type { ChatFollowMode } from "../../stores/useUIStore";
type UseViewChangeAutoFollowOptions = {
  changeKey?: string;
  messageCount: number;
  setAtBottom: (value: boolean) => void;
  setFollowMode: (mode: ChatFollowMode) => void;
  cancelAnchorRestoration: () => void;
  scheduleForceStickToBottom: () => void;
};

/** Reset detached scrolling when the user explicitly switches message views. */
export function useViewChangeAutoFollow({
  changeKey,
  messageCount,
  setAtBottom,
  setFollowMode,
  cancelAnchorRestoration,
  scheduleForceStickToBottom,
}: UseViewChangeAutoFollowOptions) {
  const previousKeyRef = useRef(changeKey);
  const pendingEmptyKeyRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    const changed = previousKeyRef.current !== changeKey;
    const becamePopulated =
      messageCount > 0 &&
      pendingEmptyKeyRef.current !== undefined &&
      pendingEmptyKeyRef.current === changeKey;
    if (!changed && !becamePopulated) return;
    if (changed) {
      cancelAnchorRestoration();
      previousKeyRef.current = changeKey;
      pendingEmptyKeyRef.current = messageCount <= 0 ? changeKey : undefined;
    } else {
      pendingEmptyKeyRef.current = undefined;
    }

    setAtBottom(true);
    setFollowMode("follow");
    if (messageCount > 0) scheduleForceStickToBottom();
  }, [
    cancelAnchorRestoration,
    changeKey,
    messageCount,
    scheduleForceStickToBottom,
    setAtBottom,
    setFollowMode,
  ]);
}
