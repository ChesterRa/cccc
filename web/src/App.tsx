import React, { useEffect, useMemo, useRef } from "react";
import { TabBar } from "./components/TabBar";
import { DropOverlay } from "./components/DropOverlay";
import { AppModals } from "./components/AppModals";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { AppHeader } from "./components/layout/AppHeader";
import { GroupSidebar } from "./components/layout/GroupSidebar";
import { useTheme } from "./hooks/useTheme";
import { useActorActions } from "./hooks/useActorActions";
import { useSSE } from "./hooks/useSSE";
import { useDragDrop } from "./hooks/useDragDrop";
import { useGroupActions } from "./hooks/useGroupActions";
import { useSwipeNavigation } from "./hooks/useSwipeNavigation";
import { useCrossGroupRecipients } from "./hooks/useCrossGroupRecipients";
import { useChatTab } from "./hooks/useChatTab";
import { useDeepLink } from "./hooks/useDeepLink";
import { useGlobalEvents } from "./hooks/useGlobalEvents";
import { useViewportHeight } from "./hooks/useViewportHeight";
import { ActorTab } from "./pages/ActorTab";
import { ChatTab } from "./pages/chat";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useComposerStore,
  useFormStore,
  useObservabilityStore,
} from "./stores";
import * as api from "./services/api";
import type { Actor } from "./types";

// ============ Main App Component ============

export default function App() {
  // Theme
  const { theme, setTheme, isDark } = useTheme();

  // Virtual keyboard viewport adjustment for mobile
  useViewportHeight();

  // Zustand stores
  const {
    groups,
    groupOrder,
    selectedGroupId,
    groupDoc,
    actors,
    groupContext,
    setSelectedGroupId,
    refreshGroups,
    refreshActors,
    loadGroup,
    openChatWindow,
    closeChatWindow,
    reorderGroups,
    getOrderedGroups,
  } = useGroupStore();

  const {
    busy,
    errorMsg,
    notice,
    isTransitioning,
    sidebarOpen,
    sidebarCollapsed,
    activeTab,
    chatUnreadCount,
    isSmallScreen,
    webReadOnly,
    showError,
    dismissError,
    dismissNotice,
    setSidebarOpen,
    toggleSidebarCollapsed,
    setActiveTab,
    setShowScrollButton,
    setChatUnreadCount,
    setSmallScreen,
    setWebReadOnly,
    sseStatus,
  } = useUIStore();

  const { openModal } = useModalStore();

  const {
    destGroupId,
    composerFiles,
    replyTarget,
    setDestGroupId,
    switchGroup,
  } = useComposerStore();

  const { setNewActorRole, setEditGroupTitle, setEditGroupTopic, setDirSuggestions } = useFormStore();

  // Actor actions hook
  const {
    getTermEpoch,
    toggleActorEnabled,
    relaunchActor,
    editActor,
    removeActor,
    openActorInbox,
  } = useActorActions(selectedGroupId);

  // Refs
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const eventContainerRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const activeTabRef = useRef<string>("chat");
  const chatAtBottomRef = useRef<boolean>(true);
  const chatScrollMemoryRef = useRef<Record<string, { atBottom: boolean; anchorId: string; offsetPx: number }>>({});
  const actorsRef = useRef<Actor[]>([]);
  const prevGroupIdRef = useRef<string | null>(null);
  // Local state
  const [showMentionMenu, setShowMentionMenu] = React.useState(false);
  const [_mentionFilter, setMentionFilter] = React.useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = React.useState(0);
  const [mountedActorIds, setMountedActorIds] = React.useState<string[]>([]);
  const [ccccHome, setCcccHome] = React.useState("");

  // Custom hooks
  const { connectStream, fetchContext, scheduleActorWarmupRefresh, cleanup: cleanupSSE } = useSSE({
    activeTabRef,
    chatAtBottomRef,
    actorsRef,
  });

  const { dropOverlayOpen, handleAppendComposerFiles, resetDragDrop, WEB_MAX_FILE_MB } = useDragDrop({
    selectedGroupId,
  });

  const { handleStartGroup, handleStopGroup, handleSetGroupState } = useGroupActions();

  // Compute sendGroupId for cross-group hooks (same logic as in useMessageActions)
  const computedSendGroupId = String(destGroupId || "").trim() || selectedGroupId;

  // Cross-group recipients hook (must be before useMessageActions which uses recipientActors)
  const {
    recipientActors,
    recipientActorsBusy,
    destGroupScopeLabel,
  } = useCrossGroupRecipients({
    actors,
    groupDoc,
    selectedGroupId,
    sendGroupId: computedSendGroupId,
  });

  // Chat tab hook (provides message actions and chat state)
  const {
    startReply,
    destGroupId: sendGroupId,
    inChatWindow,
  } = useChatTab({
    selectedGroupId,
    actors,
    recipientActors,
    composerRef,
    fileInputRef,
    chatAtBottomRef,
    chatScrollMemoryRef,
  });

  // Deep link hook
  const { parseUrlDeepLink } = useDeepLink({
    groups,
    selectedGroupId,
    setSelectedGroupId,
    setActiveTab,
    openChatWindow,
    showError,
  });

  // Global events subscription (SSE with polling fallback)
  useGlobalEvents({ refreshGroups });

  // Tab list for swipe navigation
  const allTabs = useMemo(() => {
    return ["chat", ...actors.map((a) => a.id)];
  }, [actors]);

  const handleTabChange = React.useCallback((newTab: string) => {
    // Keep Chat mounted to preserve scroll position; no need to snapshot scrollTop.
    if (newTab !== "chat") {
      setMountedActorIds((prev) => (prev.includes(newTab) ? prev : [...prev, newTab]));
    }
    setActiveTab(newTab);
  }, [setActiveTab]);

  // Swipe navigation
  const { handleTouchStart, handleTouchEnd } = useSwipeNavigation({
    tabs: allTabs,
    activeTab,
    onTabChange: handleTabChange,
  });

  // Keep refs in sync
  useEffect(() => {
    activeTabRef.current = activeTab;
    if (activeTab !== "chat") return;
    const el = eventContainerRef.current;
    if (!el) return;
    const threshold = 100;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
    chatAtBottomRef.current = atBottom;
    setShowScrollButton(!atBottom);
    if (atBottom) setChatUnreadCount(0);
  }, [activeTab, setChatUnreadCount, setShowScrollButton]);

  // Keep visited actor tabs mounted (sticky) so their terminal sessions do not reconnect/replay on tab switches.
  useEffect(() => {
    if (!activeTab || activeTab === "chat") return;
    setMountedActorIds((prev) => (prev.includes(activeTab) ? prev : [...prev, activeTab]));
  }, [activeTab]);

  useEffect(() => {
    actorsRef.current = actors;
  }, [actors]);

  // Prune mounted actor ids when the actor list changes (e.g., actor removed).
  useEffect(() => {
    const live = new Set(actors.map((a) => String(a.id || "")).filter((id) => id));
    setMountedActorIds((prev) => prev.filter((id) => live.has(id)));
  }, [actors]);

  // Responsive screen detection
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 639px)");
    const update = () => setSmallScreen(mq.matches);
    update();
    mq.addEventListener("change", update);
    return () => mq.removeEventListener("change", update);
  }, [setSmallScreen]);

  // Computed values
  const hasForeman = useMemo(() => actors.some((a) => a.role === "foreman"), [actors]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- groups and groupOrder trigger recalculation
  const orderedGroups = useMemo(() => getOrderedGroups(), [groups, groupOrder]);
  const selectedGroupMeta = useMemo(
    () => groups.find((g) => String(g.group_id || "") === selectedGroupId) || null,
    [groups, selectedGroupId]
  );
  const selectedGroupRunning = useMemo(() => {
    const anyActorRunning = actors.some((a) => !!a.running);
    return anyActorRunning || (selectedGroupMeta?.running ?? false);
  }, [actors, selectedGroupMeta]);

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

  React.useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (!destGroupId) {
      setDestGroupId(gid);
    }
  }, [destGroupId, selectedGroupId, setDestGroupId]);

  const hasReplyTarget = !!replyTarget;
  const hasComposerFiles = composerFiles.length > 0;

  React.useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (hasReplyTarget || hasComposerFiles) {
      if (sendGroupId && sendGroupId !== gid) {
        setDestGroupId(gid);
      }
    }
  }, [hasComposerFiles, hasReplyTarget, selectedGroupId, sendGroupId, setDestGroupId]);

  const renderedActorIds = useMemo(() => {
    if (activeTab !== "chat" && !mountedActorIds.includes(activeTab)) {
      return [...mountedActorIds, activeTab];
    }
    return mountedActorIds;
  }, [mountedActorIds, activeTab]);

  // ============ Group Selection Effect ============
  // Only reconnect/reload when selectedGroupId changes.
  useEffect(() => {
    // Save draft from previous group and load draft for new group
    switchGroup(prevGroupIdRef.current, selectedGroupId || null);
    prevGroupIdRef.current = selectedGroupId || null;

    if (fileInputRef.current) fileInputRef.current.value = "";
    resetDragDrop();
    setMountedActorIds([]);
    // Reset to chat tab when switching groups to avoid "Agent not found" error
    setActiveTab("chat");
    closeChatWindow();

    if (!selectedGroupId) return;

    loadGroup(selectedGroupId);
    connectStream(selectedGroupId);
    scheduleActorWarmupRefresh(selectedGroupId);

    return () => {
      cleanupSSE();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedGroupId]);

  // ============ Initial Load ============
  // Run once on mount.
  useEffect(() => {
    // Parse deep links from URL
    parseUrlDeepLink();

    refreshGroups();
    void fetchRuntimes();
    void fetchDirSuggestions();
    void useObservabilityStore.getState().load();
    void api.fetchPing().then((resp) => {
      if (resp.ok) {
        setWebReadOnly(Boolean(resp.result?.web?.read_only));
        setCcccHome(String(resp.result?.home || "").trim());
      }
    }).catch(() => {
      /* ignore */
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function fetchRuntimes() {
    const resp = await api.fetchRuntimes();
    if (resp.ok) {
      useGroupStore.getState().setRuntimes(resp.result.runtimes || []);
    }
  }

  async function fetchDirSuggestions() {
    const resp = await api.fetchDirSuggestions();
    if (resp.ok) {
      setDirSuggestions(resp.result.suggestions || []);
    }
  }

  // ============ Actions ============

  // ============ Computed for ChatTab ============

  const restoreChatAnchor = useMemo(() => {
    if (!selectedGroupId) return null;
    if (inChatWindow) return null;
    const snap = chatScrollMemoryRef.current[String(selectedGroupId || "").trim()];
    if (!snap || snap.atBottom) return null;
    if (!snap.anchorId) return null;
    return snap;
  }, [inChatWindow, selectedGroupId]);

  // ============ Render ============

  return (
    <div
      className={`w-full relative overflow-hidden ${isDark
          ? "bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950"
          : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
        }`}
      style={{ height: "calc(100% - var(--vk-offset, 0px))" }}
    >
      {/* Background orbs — hidden on mobile for GPU performance */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden hidden md:block">
        <div
          className={`absolute -top-32 -left-32 w-96 h-96 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-br from-cyan-500/20 via-cyan-600/10 to-transparent"
              : "bg-gradient-to-br from-cyan-400/25 via-cyan-500/15 to-transparent"
            }`}
          style={{ filter: "blur(60px)", willChange: "transform" }}
        />
        <div
          className={`absolute top-1/4 -right-24 w-80 h-80 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-bl from-purple-500/15 via-indigo-600/10 to-transparent"
              : "bg-gradient-to-bl from-purple-400/20 via-indigo-500/10 to-transparent"
            }`}
          style={{ filter: "blur(50px)", animationDelay: "-3s", willChange: "transform" }}
        />
        <div
          className={`absolute -bottom-20 left-1/3 w-72 h-72 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-tr from-blue-500/12 via-sky-600/8 to-transparent"
              : "bg-gradient-to-tr from-blue-400/15 via-sky-500/10 to-transparent"
            }`}
          style={{ filter: "blur(45px)", animationDelay: "-5s", willChange: "transform" }}
        />
      </div>

      {/* Noise texture — hidden on mobile for GPU performance */}
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.015] hidden md:block"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E")`,
        }}
      />

      <div className={`relative h-full md:grid transition-all duration-300 ${
        sidebarCollapsed ? "md:grid-cols-[60px_1fr]" : "md:grid-cols-[280px_1fr]"
      }`}>
        <GroupSidebar
          orderedGroups={orderedGroups}
          groupOrder={groupOrder}
          selectedGroupId={selectedGroupId}
          isOpen={sidebarOpen}
          isCollapsed={sidebarCollapsed}
          isDark={isDark}
          readOnly={webReadOnly}
          onSelectGroup={(gid) => setSelectedGroupId(gid)}
          onCreateGroup={
            webReadOnly
              ? undefined
              : () => {
                openModal("createGroup");
                void fetchDirSuggestions();
              }
          }
          onClose={() => setSidebarOpen(false)}
          onToggleCollapse={toggleSidebarCollapsed}
          onReorder={reorderGroups}
        />

        {/* Main content */}
        <main
          className={`absolute inset-0 md:relative md:inset-auto h-full flex flex-col overflow-hidden backdrop-blur-sm ${isDark ? "bg-slate-950/40" : "bg-white/60"
            }`}
        >
          <AppHeader
            isDark={isDark}
            theme={theme}
            onThemeChange={setTheme}
            webReadOnly={webReadOnly}
            selectedGroupId={selectedGroupId}
            groupDoc={groupDoc}
            selectedGroupRunning={selectedGroupRunning}
            actors={actors}
            sseStatus={sseStatus}
            busy={busy}
            errorMsg={errorMsg}
            notice={notice}
            onDismissError={dismissError}
            onNoticeAction={() => {
              dismissNotice();
            }}
            onDismissNotice={dismissNotice}
            onOpenSidebar={() => setSidebarOpen(true)}
            onOpenGroupEdit={() => {
              if (groupDoc) {
                setEditGroupTitle(groupDoc.title || "");
                setEditGroupTopic(groupDoc.topic || "");
                openModal("groupEdit");
              }
            }}
            onOpenSearch={() => openModal("search")}
            onOpenContext={() => {
              if (selectedGroupId) void fetchContext(selectedGroupId);
              openModal("context");
            }}
            onStartGroup={handleStartGroup}
            onStopGroup={handleStopGroup}
            onSetGroupState={handleSetGroupState}
            onOpenSettings={() => openModal("settings")}
            onOpenMobileMenu={() => openModal("mobileMenu")}
          />

          {/* Tab Bar */}
          {selectedGroupId && (
            <TabBar
              actors={actors}
              activeTab={activeTab}
              onTabChange={handleTabChange}
              unreadChatCount={chatUnreadCount}
              isDark={isDark}
              onAddAgent={
                webReadOnly
                  ? undefined
                  : () => {
                    setNewActorRole(hasForeman ? "peer" : "foreman");
                    openModal("addActor");
                  }
              }
              canAddAgent={!webReadOnly && !!selectedGroupId}
            />
          )}

          {/* Tab Content */}
          <div
            ref={contentRef}
            className={`relative flex-1 min-h-0 flex flex-col overflow-hidden transition-opacity duration-150 ${isTransitioning ? "opacity-0" : "opacity-100"
              }`}
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
          >
            <div
              className={`absolute inset-0 flex min-h-0 flex-col ${activeTab === "chat" ? "" : "invisible pointer-events-none"}`}
              aria-hidden={activeTab !== "chat"}
            >
              <ErrorBoundary>
              <ChatTab
                isDark={isDark}
                isSmallScreen={isSmallScreen}
                readOnly={webReadOnly}
                selectedGroupId={selectedGroupId}
                groupLabelById={groupLabelById}
                actors={actors}
                groups={groups}
                recipientActors={recipientActors}
                recipientActorsBusy={recipientActorsBusy}
                destGroupScopeLabel={destGroupScopeLabel}
                scrollRef={eventContainerRef}
                composerRef={composerRef}
                fileInputRef={fileInputRef}
                chatAtBottomRef={chatAtBottomRef}
                chatScrollMemoryRef={chatScrollMemoryRef}
                chatInitialScrollAnchorId={!inChatWindow ? restoreChatAnchor?.anchorId : undefined}
                chatInitialScrollAnchorOffsetPx={!inChatWindow ? restoreChatAnchor?.offsetPx : undefined}
                appendComposerFiles={handleAppendComposerFiles}
                onStartGroup={handleStartGroup}
                showMentionMenu={showMentionMenu}
                setShowMentionMenu={setShowMentionMenu}
                mentionSelectedIndex={mentionSelectedIndex}
                setMentionSelectedIndex={setMentionSelectedIndex}
                setMentionFilter={setMentionFilter}
              />
              </ErrorBoundary>
            </div>
            <div
              className={`absolute inset-0 flex min-h-0 flex-col ${activeTab === "chat" ? "invisible pointer-events-none" : ""}`}
              aria-hidden={activeTab === "chat"}
            >
              {renderedActorIds.map((actorId) => {
                const actor = actors.find((a) => a.id === actorId) || null;
                const isVisible = activeTab === actorId && activeTab !== "chat";
                const presence =
                  (groupContext?.presence?.agents || []).find((p) => p.id === (actor?.id || "")) || null;

                return (
                  <div key={actorId} className={isVisible ? "flex min-h-0 flex-col flex-1" : "hidden"}>
                    <ErrorBoundary>
                    <ActorTab
                      actor={actor}
                      groupId={selectedGroupId}
                      presenceAgent={presence}
                      termEpoch={actor ? getTermEpoch(actor.id) : 0}
                      busy={busy}
                      isDark={isDark}
                      isSmallScreen={isSmallScreen}
                      isVisible={isVisible}
                      readOnly={webReadOnly}
                      onToggleEnabled={() => actor && toggleActorEnabled(actor)}
                      onRelaunch={() => actor && relaunchActor(actor)}
                      onEdit={() => actor && editActor(actor)}
                      onRemove={() => actor && removeActor(actor, activeTab)}
                      onInbox={() => actor && openActorInbox(actor)}
                      onStatusChange={() => void refreshActors()}
                    />
                    </ErrorBoundary>
                  </div>
                );
              })}
            </div>
          </div>
        </main>
      </div>

      {/* Modals */}
      <AppModals
        isDark={isDark}
        ccccHome={ccccHome}
        composerRef={composerRef}
        onStartReply={startReply}
        onThemeToggle={() => setTheme(isDark ? "light" : "dark")}
        onStartGroup={handleStartGroup}
        onStopGroup={handleStopGroup}
        onSetGroupState={handleSetGroupState}
        fetchContext={fetchContext}
      />

      <DropOverlay isOpen={dropOverlayOpen} isDark={isDark} maxFileMb={WEB_MAX_FILE_MB} />
    </div>
  );
}
