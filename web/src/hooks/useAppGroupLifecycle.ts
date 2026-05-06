import { useEffect, useLayoutEffect, useRef } from "react";

type UseAppGroupLifecycleOptions = {
  selectedGroupId: string;
  destGroupId: string;
  sendGroupId: string;
  hasReplyTarget: boolean;
  hasComposerFiles: boolean;
  setDestGroupId: (groupId: string) => void;
  switchGroup: (prevGroupId: string | null, nextGroupId: string | null) => void;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  resetDragDrop: () => void;
  resetMountedActorIds: () => void;
  setActiveTab: (tab: string) => void;
  closeChatWindow: () => void;
  loadGroup: (groupId: string) => void;
  connectStream: (groupId: string) => void;
  cleanupSSE: () => void;
};

export function useAppGroupLifecycle({
  selectedGroupId,
  destGroupId,
  sendGroupId,
  hasReplyTarget,
  hasComposerFiles,
  setDestGroupId,
  switchGroup,
  fileInputRef,
  resetDragDrop,
  resetMountedActorIds,
  setActiveTab,
  closeChatWindow,
  loadGroup,
  connectStream,
  cleanupSSE,
}: UseAppGroupLifecycleOptions) {
  const prevGroupIdRef = useRef<string | null>(null);

  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (!destGroupId) {
      setDestGroupId(gid);
    }
  }, [destGroupId, selectedGroupId, setDestGroupId]);

  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (hasReplyTarget || hasComposerFiles) {
      if (sendGroupId && sendGroupId !== gid) {
        setDestGroupId(gid);
      }
    }
  }, [hasComposerFiles, hasReplyTarget, selectedGroupId, sendGroupId, setDestGroupId]);

  // Move composer ownership before the group switch renders, avoiding a first-frame
  // read from the previous group's draft.
  useLayoutEffect(() => {
    switchGroup(prevGroupIdRef.current, selectedGroupId || null);
    prevGroupIdRef.current = selectedGroupId || null;
  }, [selectedGroupId, switchGroup]);

  useEffect(() => {
    if (fileInputRef.current) fileInputRef.current.value = "";
    resetDragDrop();
    resetMountedActorIds();
    setActiveTab("chat");
    closeChatWindow();

    if (!selectedGroupId) return;

    loadGroup(selectedGroupId);
    connectStream(selectedGroupId);

    return () => {
      cleanupSSE();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedGroupId]);
}
