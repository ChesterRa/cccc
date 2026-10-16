import { useEffect } from "react";

type UseAppGroupLifecycleOptions = {
  selectedGroupId: string;
  destGroupId: string;
  sendGroupId: string;
  hasReplyTarget: boolean;
  setDestGroupId: (groupId: string) => void;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  resetDragDrop: () => void;
  setActiveTab: (tab: string) => void;
  closeChatWindow: (groupId?: string) => void;
  loadGroup: (groupId: string) => void;
  connectStream: (groupId: string) => void;
  cleanupSSE: () => void;
};

export function shouldResetDestGroupForLifecycle({
  selectedGroupId,
  destGroupId,
  sendGroupId,
  hasReplyTarget,
}: {
  selectedGroupId: string;
  destGroupId: string;
  sendGroupId: string;
  hasReplyTarget: boolean;
}): boolean {
  const gid = String(selectedGroupId || "").trim();
  if (!gid) return false;
  const dest = String(destGroupId || "").trim();
  const send = String(sendGroupId || "").trim();
  if (!dest) return true;
  // Attachments travel cross-group; only a reply pins the selected Group.
  return hasReplyTarget && !!send && send !== gid;
}

export function useAppGroupLifecycle({
  selectedGroupId,
  destGroupId,
  sendGroupId,
  hasReplyTarget,
  setDestGroupId,
  fileInputRef,
  resetDragDrop,
  setActiveTab,
  closeChatWindow,
  loadGroup,
  connectStream,
  cleanupSSE,
}: UseAppGroupLifecycleOptions) {
  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (
      shouldResetDestGroupForLifecycle({
        selectedGroupId: gid,
        destGroupId,
        sendGroupId,
        hasReplyTarget,
      })
    ) {
      setDestGroupId(gid);
    }
  }, [destGroupId, hasReplyTarget, selectedGroupId, sendGroupId, setDestGroupId]);

  useEffect(() => {
    if (fileInputRef.current) fileInputRef.current.value = "";
    resetDragDrop();
    setActiveTab("chat");

    if (!selectedGroupId) return;

    loadGroup(selectedGroupId);
    connectStream(selectedGroupId);

    return () => {
      // Retire the Group being left, not a message window explicitly opened
      // in the destination while this selection change was committing.
      closeChatWindow(selectedGroupId);
      cleanupSSE();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedGroupId]);
}
