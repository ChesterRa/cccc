import React, { useEffect, useMemo, useRef } from "react";
import { useGroupStore, useUIStore } from "../stores";
import { requestWorkspaceNavigation } from "../stores/workspaceNavigation";
import { groupMessagesVisible } from "../stores/useUIStore";
import type { Actor } from "../types";

export function isChatViewportAtBottom(
  scrollHeight: number,
  scrollTop: number,
  clientHeight: number,
  threshold = 100,
): boolean {
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

export type CreatedDotsNavigation = { groupId: string; actorId: string; activeTab: string };

type UseAppTabStateResult = {
  composerRef: React.RefObject<HTMLTextAreaElement | null>;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  eventContainerRef: React.MutableRefObject<HTMLDivElement | null>;
  contentRef: React.MutableRefObject<HTMLDivElement | null>;
  activeTabRef: React.MutableRefObject<string>;
  chatAtBottomRef: React.MutableRefObject<boolean>;
  actorsRef: React.MutableRefObject<Actor[]>;
  allTabs: string[];
  handleTabChange: (newTab: string) => void;
  onDotsActorCreated: (visit: CreatedDotsNavigation) => void;
};

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

  const messagesVisible = useUIStore((state) => groupMessagesVisible(selectedGroupId, state));

  // Creation modals unmount before delayed Actor hydration; this app-level owner remains.
  const pending = useRef<CreatedDotsNavigation | null>(null);
  const openCreatedDots = React.useCallback(() => {
    const visit = pending.current;
    if (!visit) return;
    const group = useGroupStore.getState();
    if (
      group.selectedGroupId !== visit.groupId ||
      useUIStore.getState().activeTab !== visit.activeTab
    ) {
      pending.current = null;
      return;
    }
    if (
      !group.actors.some((actor) => actor.id === visit.actorId && actor.runtime === "chatgpt_dots")
    )
      return;
    pending.current = null;
    requestWorkspaceNavigation(() => {
      if (
        useGroupStore.getState().selectedGroupId !== visit.groupId ||
        useUIStore.getState().activeTab !== visit.activeTab
      )
        return;
      useUIStore.getState().setActiveTab(visit.actorId);
      useUIStore.getState().setChatMobileSurface(visit.groupId, "messages");
    });
  }, []);

  useEffect(() => {
    openCreatedDots();
  }, [runtimeActors, selectedGroupId, activeTab, openCreatedDots]);

  const onDotsActorCreated = React.useCallback(
    (visit: CreatedDotsNavigation) => {
      pending.current = visit;
      openCreatedDots();
    },
    [openCreatedDots],
  );

  const allTabs = useMemo(() => ["chat"], []);

  const handleTabChange = React.useCallback(
    (newTab: string) => {
      setActiveTab(newTab);
    },
    [setActiveTab],
  );

  useEffect(() => {
    activeTabRef.current = activeTab;
    if (!messagesVisible) return;
    if (!selectedGroupId) return;
    const el = eventContainerRef.current;
    if (!el) return;

    const atBottom = isChatViewportAtBottom(el.scrollHeight, el.scrollTop, el.clientHeight);
    chatAtBottomRef.current = atBottom;
    setShowScrollButton(selectedGroupId, !atBottom);
    if (atBottom) setChatUnreadCount(selectedGroupId, 0);
  }, [activeTab, messagesVisible, selectedGroupId, setChatUnreadCount, setShowScrollButton]);

  useEffect(() => {
    if (!messagesVisible) return;
    if (isSmallScreen) return;
    requestAnimationFrame(() => composerRef.current?.focus());
  }, [activeTab, messagesVisible, isSmallScreen]);

  useEffect(() => {
    actorsRef.current = runtimeActors;
  }, [runtimeActors]);

  return {
    composerRef,
    fileInputRef,
    eventContainerRef,
    contentRef,
    activeTabRef,
    chatAtBottomRef,
    actorsRef,
    allTabs,
    handleTabChange,
    onDotsActorCreated,
  };
}
