import React, { useEffect, useMemo, useRef, useState } from "react";
import type { Actor } from "../types";

export function isChatViewportAtBottom(scrollHeight: number, scrollTop: number, clientHeight: number, threshold = 100): boolean {
  return scrollHeight - scrollTop - clientHeight < threshold;
}

type UseAppTabStateOptions = {
  activeTab: string;
  runtimeActors: Actor[];
  selectedGroupId: string;
  isSmallScreen: boolean;
  setActiveTab: (tab: string) => void;
  setShowScrollButton: (groupId: string, value: boolean) => void;
  setChatUnreadCount: (groupId: string, value: number) => void;
};

type UseAppTabStateResult = {
  composerRef: React.RefObject<HTMLTextAreaElement | null>;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  eventContainerRef: React.MutableRefObject<HTMLDivElement | null>;
  contentRef: React.MutableRefObject<HTMLDivElement | null>;
  activeTabRef: React.MutableRefObject<string>;
  chatAtBottomRef: React.MutableRefObject<boolean>;
  actorsRef: React.MutableRefObject<Actor[]>;
  allTabs: string[];
  renderedActorIds: string[];
  resetMountedActorIds: () => void;
  handleTabChange: (newTab: string) => void;
};

export function getRenderedActorIds({
  mountedActorIds,
  activeTab,
  runtimeActors,
}: {
  mountedActorIds: string[];
  activeTab: string;
  runtimeActors: Actor[];
}): string[] {
  const live = new Set(runtimeActors.map((actor) => String(actor.id || "").trim()).filter((id) => id));
  const mountedLiveIds = mountedActorIds.filter((id) => live.has(id));
  if (activeTab === "chat") return mountedLiveIds;
  if (mountedLiveIds.includes(activeTab)) return mountedLiveIds;
  if (mountedActorIds.includes(activeTab) || live.has(activeTab)) return [...mountedLiveIds, activeTab];
  return mountedLiveIds;
}

export function useAppTabState({
  activeTab,
  runtimeActors,
  selectedGroupId,
  isSmallScreen,
  setActiveTab,
  setShowScrollButton,
  setChatUnreadCount,
}: UseAppTabStateOptions): UseAppTabStateResult {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const eventContainerRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const activeTabRef = useRef<string>("chat");
  const chatAtBottomRef = useRef<boolean>(true);
  const actorsRef = useRef<Actor[]>([]);
  const [mountedActorIds, setMountedActorIds] = useState<string[]>([]);

  const allTabs = useMemo(() => ["chat"], []);

  const handleTabChange = React.useCallback(
    (newTab: string) => {
      if (newTab !== "chat") {
        setMountedActorIds((prev) => (prev.includes(newTab) ? prev : [...prev, newTab]));
      }
      setActiveTab(newTab);
    },
    [setActiveTab]
  );

  useEffect(() => {
    activeTabRef.current = activeTab;
    if (activeTab !== "chat") return;
    if (!selectedGroupId) return;
    const el = eventContainerRef.current;
    if (!el) return;

    const atBottom = isChatViewportAtBottom(el.scrollHeight, el.scrollTop, el.clientHeight);
    chatAtBottomRef.current = atBottom;
    setShowScrollButton(selectedGroupId, !atBottom);
    if (atBottom) setChatUnreadCount(selectedGroupId, 0);
  }, [activeTab, selectedGroupId, setChatUnreadCount, setShowScrollButton]);

  useEffect(() => {
    if (activeTab !== "chat") return;
    if (isSmallScreen) return;
    requestAnimationFrame(() => composerRef.current?.focus());
  }, [activeTab, isSmallScreen]);

  useEffect(() => {
    actorsRef.current = runtimeActors;
  }, [runtimeActors]);

  const renderedActorIds = useMemo(() => {
    return getRenderedActorIds({ mountedActorIds, activeTab, runtimeActors });
  }, [mountedActorIds, activeTab, runtimeActors]);

  const resetMountedActorIds = React.useCallback(() => {
    setMountedActorIds([]);
  }, []);

  return {
    composerRef,
    fileInputRef,
    eventContainerRef,
    contentRef,
    activeTabRef,
    chatAtBottomRef,
    actorsRef,
    allTabs,
    renderedActorIds,
    resetMountedActorIds,
    handleTabChange,
  };
}
