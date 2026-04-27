import React, { lazy, Suspense, useEffect, useMemo } from "react";
import { DropOverlay } from "./components/DropOverlay";
const AppModals = lazy(() => import("./components/AppModals").then((m) => ({ default: m.AppModals })));
const WebPet = lazy(() => import("./features/webPet/WebPet").then((m) => ({ default: m.WebPet })));
import { AppBackground } from "./components/app/AppBackground";
import { AppFeedback } from "./components/app/AppFeedback";
import { AppShell } from "./components/app/AppShell";
import { useTextScale } from "./hooks/useTextScale";
import { useTheme } from "./hooks/useTheme";
import { useActorActions } from "./hooks/useActorActions";
import { useSelectedGroupRuntime } from "./hooks/useSelectedGroupRuntime";
import { useSSE } from "./hooks/useSSE";
import { useDragDrop } from "./hooks/useDragDrop";
import { useGroupActions } from "./hooks/useGroupActions";
import { useSwipeNavigation } from "./hooks/useSwipeNavigation";
import { useCrossGroupRecipients } from "./hooks/useCrossGroupRecipients";
import { useDeepLink } from "./hooks/useDeepLink";
import { useGlobalEvents } from "./hooks/useGlobalEvents";
import { useViewportHeight } from "./hooks/useViewportHeight";
import { useAppChrome } from "./hooks/useAppChrome";
import { useAppGroupLifecycle } from "./hooks/useAppGroupLifecycle";
import { useAppTabState } from "./hooks/useAppTabState";
import { getEffectiveComposerDestGroupId } from "./stores/useComposerStore";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useComposerStore,
  useFormStore,
  useObservabilityStore,
} from "./stores";
import { useChatOutboxStore } from "./stores/chatOutboxStore";
import type { ChatMessageData, LedgerEvent } from "./types";
import { filterVisibleRuntimeActors } from "./utils/runtimeVisibility";

// ============ Main App Component ============

export default function App() {
  // Theme
  const { theme, setTheme, isDark } = useTheme();
  const { textScale, setTextScale } = useTextScale();

  // Virtual keyboard viewport adjustment for mobile
  useViewportHeight();

  // Zustand stores
  const groups = useGroupStore((state) => state.groups);
  const archivedGroupIds = useGroupStore((state) => state.archivedGroupIds);
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  const groupDoc = useGroupStore((state) => state.groupDoc);
  const actors = useGroupStore((state) => state.actors);
  const internalRuntimeActorsByGroup = useGroupStore((state) => state.internalRuntimeActorsByGroup);
  const groupContext = useGroupStore((state) => state.groupContext);
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const selectedGroupActorsHydrating = useGroupStore((state) => state.selectedGroupActorsHydrating);
  const setSelectedGroupId = useGroupStore((state) => state.setSelectedGroupId);
  const refreshGroups = useGroupStore((state) => state.refreshGroups);
  const refreshActors = useGroupStore((state) => state.refreshActors);
  const refreshInternalRuntimeActors = useGroupStore((state) => state.refreshInternalRuntimeActors);
  const loadGroup = useGroupStore((state) => state.loadGroup);
  const warmGroup = useGroupStore((state) => state.warmGroup);
  const openChatWindow = useGroupStore((state) => state.openChatWindow);
  const closeChatWindow = useGroupStore((state) => state.closeChatWindow);
  const reorderGroupsInSection = useGroupStore((state) => state.reorderGroupsInSection);
  const archiveGroup = useGroupStore((state) => state.archiveGroup);
  const restoreGroup = useGroupStore((state) => state.restoreGroup);
  const getOrderedGroups = useGroupStore((state) => state.getOrderedGroups);

  const busy = useUIStore((s) => s.busy);
  const errorMsg = useUIStore((s) => s.errorMsg);
  const notice = useUIStore((s) => s.notice);
  const isTransitioning = useUIStore((s) => s.isTransitioning);
  const sidebarOpen = useUIStore((s) => s.sidebarOpen);
  const sidebarCollapsed = useUIStore((s) => s.sidebarCollapsed);
  const sidebarWidth = useUIStore((s) => s.sidebarWidth);
  const activeTab = useUIStore((s) => s.activeTab);
  const isSmallScreen = useUIStore((s) => s.isSmallScreen);
  const webReadOnly = useUIStore((s) => s.webReadOnly);
  const showError = useUIStore((s) => s.showError);
  const dismissError = useUIStore((s) => s.dismissError);
  const dismissNotice = useUIStore((s) => s.dismissNotice);
  const setSidebarOpen = useUIStore((s) => s.setSidebarOpen);
  const toggleSidebarCollapsed = useUIStore((s) => s.toggleSidebarCollapsed);
  const setSidebarWidth = useUIStore((s) => s.setSidebarWidth);
  const setActiveTab = useUIStore((s) => s.setActiveTab);
  const setShowScrollButton = useUIStore((s) => s.setShowScrollButton);
  const setChatUnreadCount = useUIStore((s) => s.setChatUnreadCount);
  const setSmallScreen = useUIStore((s) => s.setSmallScreen);
  const setWebReadOnly = useUIStore((s) => s.setWebReadOnly);
  const sseStatus = useUIStore((s) => s.sseStatus);

  const openModal = useModalStore((s) => s.openModal);
  const modalFlags = useModalStore((s) => s.modals);
  const editingActor = useModalStore((s) => s.editingActor);
  const peerRuntimeVisibility = useObservabilityStore((state) => state.peerRuntimeVisibility);
  const petRuntimeVisibility = useObservabilityStore((state) => state.petRuntimeVisibility);

  const {
    activeGroupId,
    destGroupId,
    composerFiles,
    replyTarget,
    setDestGroupId,
    setReplyTarget,
    setToText,
    switchGroup,
  } = useComposerStore();

  const { setEditGroupTitle, setEditGroupTopic, setDirSuggestions } = useFormStore();
  const clearAllOutbox = useChatOutboxStore((state) => state.clearAll);

  // Actor actions hook
  const {
    getTermEpoch,
    toggleActorEnabled,
    relaunchActor,
    editActor,
    removeActor,
    openActorInbox,
  } = useActorActions(selectedGroupId);

  const [showMentionMenu, setShowMentionMenu] = React.useState(false);
  const [_mentionFilter, setMentionFilter] = React.useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = React.useState(0);
  const internalRuntimeActors = useMemo(
    () => internalRuntimeActorsByGroup[String(selectedGroupId || "").trim()] || [],
    [internalRuntimeActorsByGroup, selectedGroupId]
  );
  const visibleRuntimeActors = useMemo(
    () =>
      filterVisibleRuntimeActors(
        [
          ...actors,
          ...internalRuntimeActors.filter(
            (actor) => !actors.some((existing) => String(existing.id || "") === String(actor.id || ""))
          ),
        ],
        {
        peerRuntimeVisibility,
        petRuntimeVisibility,
      }
      ),
    [actors, internalRuntimeActors, peerRuntimeVisibility, petRuntimeVisibility]
  );

  useEffect(() => {
    const handlePageHide = () => clearAllOutbox();
    window.addEventListener("pagehide", handlePageHide);
    return () => {
      window.removeEventListener("pagehide", handlePageHide);
      clearAllOutbox();
    };
  }, [clearAllOutbox]);

  const {
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
  } = useAppTabState({
    activeTab,
    actors,
    runtimeActors: visibleRuntimeActors,
    selectedGroupId,
    isSmallScreen,
    setActiveTab,
    setShowScrollButton,
    setChatUnreadCount,
  });

  useEffect(() => {
    if (activeTab === "chat") return;
    if (visibleRuntimeActors.some((actor) => String(actor.id || "") === activeTab)) return;
    setActiveTab("chat");
  }, [activeTab, setActiveTab, visibleRuntimeActors]);

  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || petRuntimeVisibility !== "visible") {
      return undefined;
    }
    void refreshInternalRuntimeActors(gid);
    const interval = window.setInterval(() => {
      void refreshInternalRuntimeActors(gid);
    }, 60000);
    return () => window.clearInterval(interval);
  }, [selectedGroupId, petRuntimeVisibility, refreshInternalRuntimeActors]);

  // Custom hooks
  const { connectStream, fetchContext, cleanup: cleanupSSE } = useSSE({
    activeTabRef,
    chatAtBottomRef,
    actorsRef,
  });

  const { dropOverlayOpen, handleAppendComposerFiles, resetDragDrop, WEB_MAX_FILE_MB } = useDragDrop({
    selectedGroupId,
  });

  const { handleStartGroup, handleStopGroup, handleSetGroupState } = useGroupActions();

  const computedSendGroupId = getEffectiveComposerDestGroupId(destGroupId, activeGroupId, selectedGroupId);

  const { recipientActors, recipientActorsBusy, destGroupScopeLabel } = useCrossGroupRecipients({
    actors,
    groupDoc,
    selectedGroupId,
    composerGroupId: activeGroupId,
    sendGroupId: computedSendGroupId,
  });
  const sendGroupId = computedSendGroupId;

  const startReply = React.useCallback(
    (ev: LedgerEvent) => {
      if (!ev.id || ev.kind !== "chat.message") return;
      const data = ev.data as ChatMessageData | undefined;
      const text = data?.text ? String(data.text) : "";

      if (selectedGroupId) {
        setDestGroupId(selectedGroupId);
      }

      const by = String(ev.by || "").trim();
      const authorIsActor = by && by !== "user" && actors.some((a) => String(a.id || "") === by);
      const originalTo = Array.isArray(data?.to)
        ? data.to.map((token) => String(token || "").trim()).filter((token) => token)
        : [];
      const policy = groupSettings?.default_send_to || "foreman";
      const defaultTo = authorIsActor
        ? [by]
        : originalTo.length > 0
          ? originalTo
          : policy === "foreman"
            ? ["@foreman"]
            : [];
      setToText(defaultTo.join(", "));

      setReplyTarget({
        eventId: String(ev.id),
        by: String(ev.by || "unknown"),
        text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
      });
      requestAnimationFrame(() => composerRef.current?.focus());
    },
    [selectedGroupId, actors, composerRef, groupSettings, setDestGroupId, setReplyTarget, setToText]
  );

  const { parseUrlDeepLink } = useDeepLink({
    groups,
    selectedGroupId,
    setSelectedGroupId,
    setActiveTab,
    openChatWindow,
    showError,
  });

  useGlobalEvents({
    refreshGroups,
    refreshActors,
    selectedGroupId,
  });

  const { canManageGroups, ccccHome, fetchDirSuggestions } = useAppChrome({
    parseUrlDeepLink,
    refreshGroups,
    setWebReadOnly,
    setSmallScreen,
    showError,
    setDirSuggestions,
    groupEditOpen: modalFlags.groupEdit,
    addActorOpen: modalFlags.addActor,
    editingActor,
  });

  const { handleTouchStart, handleTouchEnd } = useSwipeNavigation({
    tabs: allTabs,
    activeTab,
    onTabChange: handleTabChange,
  });

  const {
    selectedGroupRunning,
    selectedGroupRuntimeStatus,
  } = useSelectedGroupRuntime({
    groups,
    selectedGroupId,
    groupDoc,
    actors,
  });
  const orderedGroups = getOrderedGroups();

  const groupLabelById = useMemo(() => {
    const out: Record<string, string> = {};
    for (const g of groups || []) {
      const gid = String(g.group_id || "").trim();
      if (!gid) continue;
      const title = String(g.title || "").trim();
      out[gid] = title || gid;
    }
    return out;
  }, [groups]);

  const hasReplyTarget = !!replyTarget;
  const hasComposerFiles = composerFiles.length > 0;

  useAppGroupLifecycle({
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
  });

  return (
    <div
      className={`relative min-h-0 w-full overflow-hidden ${
        isDark ? "bg-black text-slate-100" : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
      }`}
      style={{
        height: "calc(100dvh - var(--vk-offset, 0px))",
        maxHeight: "calc(100dvh - var(--vk-offset, 0px))",
      }}
    >
      <AppBackground isDark={isDark} />

      <AppShell
        orderedGroups={orderedGroups}
        archivedGroupIds={archivedGroupIds}
        groups={groups}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        groupContext={groupContext}
        actors={actors}
        runtimeActors={visibleRuntimeActors}
        recipientActors={recipientActors}
        recipientActorsBusy={recipientActorsBusy}
        destGroupScopeLabel={destGroupScopeLabel}
        renderedActorIds={renderedActorIds}
        activeTab={activeTab}
        busy={busy}
        isTransitioning={isTransitioning}
        sidebarOpen={sidebarOpen}
        sidebarCollapsed={sidebarCollapsed}
        sidebarWidth={sidebarWidth}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        webReadOnly={webReadOnly}
        selectedGroupRunning={selectedGroupRunning}
        selectedGroupRuntimeStatus={selectedGroupRuntimeStatus}
        selectedGroupActorsHydrating={selectedGroupActorsHydrating}
        theme={theme}
        textScale={textScale}
        sseStatus={sseStatus}
        groupLabelById={groupLabelById}
        mentionSelectedIndex={mentionSelectedIndex}
        showMentionMenu={showMentionMenu}
        composerRef={composerRef}
        fileInputRef={fileInputRef}
        eventContainerRef={eventContainerRef}
        contentRef={contentRef}
        chatAtBottomRef={chatAtBottomRef}
        onThemeChange={setTheme}
        onTextScaleChange={setTextScale}
        onSelectGroup={setSelectedGroupId}
        onWarmGroup={(gid) => void warmGroup(gid)}
        onCreateGroup={
          !webReadOnly && canManageGroups
            ? () => {
                openModal("createGroup");
                void fetchDirSuggestions();
              }
            : undefined
        }
        onCloseSidebar={() => setSidebarOpen(false)}
        onToggleSidebar={toggleSidebarCollapsed}
        onResizeSidebar={setSidebarWidth}
        onReorderGroupsInSection={reorderGroupsInSection}
        onArchiveGroup={archiveGroup}
        onRestoreGroup={restoreGroup}
        onOpenSidebar={() => setSidebarOpen(true)}
        onOpenGroupEdit={
          canManageGroups
            ? () => {
                if (groupDoc) {
                  setEditGroupTitle(groupDoc.title || "");
                  setEditGroupTopic(groupDoc.topic || "");
                  openModal("groupEdit");
                }
              }
            : undefined
        }
        onOpenSearch={() => openModal("search")}
        onOpenContext={() => {
          if (selectedGroupId && !groupContext) void fetchContext(selectedGroupId);
          openModal("context");
        }}
        onStartGroup={handleStartGroup}
        onStopGroup={handleStopGroup}
        onSetGroupState={handleSetGroupState}
        onOpenSettings={() => openModal("settings")}
        onOpenMobileMenu={() => openModal("mobileMenu")}
        onTabChange={handleTabChange}
        appendComposerFiles={handleAppendComposerFiles}
        setMentionFilter={setMentionFilter}
        setMentionSelectedIndex={setMentionSelectedIndex}
        setShowMentionMenu={setShowMentionMenu}
        getTermEpoch={getTermEpoch}
        onToggleActorEnabled={toggleActorEnabled}
        onRelaunchActor={relaunchActor}
        onEditActor={editActor}
        onRemoveActor={removeActor}
        onOpenActorInbox={openActorInbox}
        onRefreshActors={() => void refreshActors()}
        onTouchStart={handleTouchStart}
        onTouchEnd={handleTouchEnd}
      />

      {selectedGroupId ? (
        <Suspense fallback={null}>
          <WebPet groupId={selectedGroupId} />
        </Suspense>
      ) : null}

      <AppFeedback
        isDark={isDark}
        webReadOnly={webReadOnly}
        errorMsg={errorMsg}
        notice={notice}
        dismissError={dismissError}
        dismissNotice={dismissNotice}
      />

      <Suspense fallback={null}>
        <AppModals
          isDark={isDark}
          theme={theme}
          textScale={textScale}
          readOnly={webReadOnly}
          ccccHome={ccccHome}
          composerRef={composerRef}
          onStartReply={startReply}
          onThemeChange={setTheme}
          onTextScaleChange={setTextScale}
          onStartGroup={handleStartGroup}
          onStopGroup={handleStopGroup}
          onSetGroupState={handleSetGroupState}
          fetchContext={fetchContext}
          canManageGroups={canManageGroups}
        />
      </Suspense>

      <DropOverlay isOpen={dropOverlayOpen} isDark={isDark} maxFileMb={WEB_MAX_FILE_MB} />
    </div>
  );
}
