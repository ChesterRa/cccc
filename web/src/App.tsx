import React, { lazy, Suspense, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { TabBar } from "./components/TabBar";
import { DropOverlay } from "./components/DropOverlay";
const AppModals = lazy(() => import("./components/AppModals").then((m) => ({ default: m.AppModals })));
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
import { useDeepLink } from "./hooks/useDeepLink";
import { useGlobalEvents } from "./hooks/useGlobalEvents";
import { useViewportHeight } from "./hooks/useViewportHeight";
import { getChatSession } from "./stores/useUIStore";
import { classNames } from "./utils/classNames";
import { ActorTab } from "./pages/ActorTab";
import { ChatTab } from "./pages/chat";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useComposerStore,
  useFormStore,
} from "./stores";
import * as api from "./services/api";
import type { Actor, ChatMessageData, LedgerEvent } from "./types";

// ============ Main App Component ============

export default function App() {
  const { t } = useTranslation(["layout", "common"]);

  // Theme
  const { theme, setTheme, isDark } = useTheme();

  // Virtual keyboard viewport adjustment for mobile
  useViewportHeight();

  // Zustand stores
  const groups = useGroupStore((state) => state.groups);
  const groupOrder = useGroupStore((state) => state.groupOrder);
  const selectedGroupId = useGroupStore((state) => state.selectedGroupId);
  const groupDoc = useGroupStore((state) => state.groupDoc);
  const actors = useGroupStore((state) => state.actors);
  const groupContext = useGroupStore((state) => state.groupContext);
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const setSelectedGroupId = useGroupStore((state) => state.setSelectedGroupId);
  const refreshGroups = useGroupStore((state) => state.refreshGroups);
  const refreshActors = useGroupStore((state) => state.refreshActors);
  const loadGroup = useGroupStore((state) => state.loadGroup);
  const warmGroup = useGroupStore((state) => state.warmGroup);
  const openChatWindow = useGroupStore((state) => state.openChatWindow);
  const closeChatWindow = useGroupStore((state) => state.closeChatWindow);
  const reorderGroups = useGroupStore((state) => state.reorderGroups);
  const getOrderedGroups = useGroupStore((state) => state.getOrderedGroups);

  const busy = useUIStore((s) => s.busy);
  const errorMsg = useUIStore((s) => s.errorMsg);
  const notice = useUIStore((s) => s.notice);
  const isTransitioning = useUIStore((s) => s.isTransitioning);
  const sidebarOpen = useUIStore((s) => s.sidebarOpen);
  const sidebarCollapsed = useUIStore((s) => s.sidebarCollapsed);
  const activeTab = useUIStore((s) => s.activeTab);
  const chatSessions = useUIStore((s) => s.chatSessions);
  const isSmallScreen = useUIStore((s) => s.isSmallScreen);
  const webReadOnly = useUIStore((s) => s.webReadOnly);
  const showError = useUIStore((s) => s.showError);
  const dismissError = useUIStore((s) => s.dismissError);
  const dismissNotice = useUIStore((s) => s.dismissNotice);
  const setSidebarOpen = useUIStore((s) => s.setSidebarOpen);
  const toggleSidebarCollapsed = useUIStore((s) => s.toggleSidebarCollapsed);
  const setActiveTab = useUIStore((s) => s.setActiveTab);
  const setShowScrollButton = useUIStore((s) => s.setShowScrollButton);
  const setChatUnreadCount = useUIStore((s) => s.setChatUnreadCount);
  const setSmallScreen = useUIStore((s) => s.setSmallScreen);
  const setWebReadOnly = useUIStore((s) => s.setWebReadOnly);
  const sseStatus = useUIStore((s) => s.sseStatus);

  const openModal = useModalStore((s) => s.openModal);
  const modalFlags = useModalStore((s) => s.modals);
  const editingActor = useModalStore((s) => s.editingActor);

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
  const actorsRef = useRef<Actor[]>([]);
  const chatSession = useMemo(
    () => getChatSession(selectedGroupId, chatSessions),
    [selectedGroupId, chatSessions]
  );
  const chatUnreadCount = chatSession.chatUnreadCount;
  const chatSessionAtBottom = chatSession.scrollSnapshot?.atBottom;

  const prevGroupIdRef = useRef<string | null>(null);
  // Local state
  const [showMentionMenu, setShowMentionMenu] = React.useState(false);
  const [_mentionFilter, setMentionFilter] = React.useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = React.useState(0);
  const [mountedActorIds, setMountedActorIds] = React.useState<string[]>([]);
  const [ccccHome, setCcccHome] = React.useState("");
  const [daemonVersion, setDaemonVersion] = React.useState("");
  const [canAccessGlobalSettings, setCanAccessGlobalSettings] = React.useState<boolean | null>(null);

  // Custom hooks
  const { connectStream, fetchContext, contextRefreshTimerRef, cleanup: cleanupSSE } = useSSE({
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
    composerGroupId: activeGroupId,
    sendGroupId: computedSendGroupId,
  });
  const sendGroupId = computedSendGroupId;

  const startReply = React.useCallback((ev: LedgerEvent) => {
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
    const defaultTo =
      authorIsActor
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
  }, [selectedGroupId, actors, groupSettings, setDestGroupId, setReplyTarget, setToText]);

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

  const refreshWebAccessSession = React.useCallback(async () => {
    try {
      const resp = await api.fetchWebAccessSession();
      const session = resp.ok ? resp.result?.web_access_session ?? null : null;
      const allowed = Boolean(session?.can_access_global_settings ?? !(session?.login_active ?? false));
      setCanAccessGlobalSettings(allowed);
    } catch {
      setCanAccessGlobalSettings(null);
    }
  }, []);

  useEffect(() => {
    void refreshWebAccessSession();
    const handleFocus = () => {
      void refreshWebAccessSession();
    };
    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [refreshWebAccessSession]);

  // Tab list for swipe navigation
  const canManageGroups = canAccessGlobalSettings === true;

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

  // Keep refs in sync + scroll to bottom when returning to chat tab
  useEffect(() => {
    activeTabRef.current = activeTab;
    if (activeTab !== "chat") return;
    if (!selectedGroupId) return;
    const el = eventContainerRef.current;
    if (!el) return;

    // If user was at bottom before switching away, scroll to bottom on return
    // (new messages may have arrived while on another tab)
    if (chatSessionAtBottom ?? chatAtBottomRef.current) {
      chatAtBottomRef.current = true;
      requestAnimationFrame(() => {
        el.scrollTo({ top: el.scrollHeight, behavior: "auto" });
      });
      setShowScrollButton(selectedGroupId, false);
      setChatUnreadCount(selectedGroupId, 0);
      return;
    }

    const threshold = 100;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
    chatAtBottomRef.current = atBottom;
    setShowScrollButton(selectedGroupId, !atBottom);
    if (atBottom) setChatUnreadCount(selectedGroupId, 0);
  }, [activeTab, selectedGroupId, chatSessionAtBottom, setChatUnreadCount, setShowScrollButton]);

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
    void api.fetchPing().then((resp) => {
      if (resp.ok) {
        setWebReadOnly(Boolean(resp.result?.web?.read_only));
        setDaemonVersion(String(resp.result?.daemon?.version || "").trim());
      }
    }).catch(() => {
      /* ignore */
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const ensureRuntimesLoaded = React.useCallback(async () => {
    if (useGroupStore.getState().runtimes.length > 0) return;
    try {
      const resp = await api.fetchRuntimes();
      if (resp.ok) {
        useGroupStore.getState().setRuntimes(resp.result.runtimes || []);
        return;
      }
      showError(resp.error?.message || "Failed to load runtimes");
    } catch {
      showError("Failed to load runtimes");
    }
  }, [showError]);

  const fetchDirSuggestions = React.useCallback(async () => {
    try {
      const resp = await api.fetchDirSuggestions();
      if (resp.ok) {
        setDirSuggestions(resp.result.suggestions || []);
        return;
      }
      showError(resp.error?.message || "Failed to load directories");
    } catch {
      showError("Failed to load directories");
    }
  }, [setDirSuggestions, showError]);

  const loadCcccHome = React.useCallback(async () => {
    if (ccccHome) return;
    try {
      const resp = await api.fetchPing({ includeHome: true });
      if (resp.ok) {
        setCcccHome(String(resp.result?.home || "").trim());
      }
    } catch {
      /* ignore */
    }
  }, [ccccHome]);

  useEffect(() => {
    if (!modalFlags.groupEdit) return;
    void loadCcccHome();
  }, [loadCcccHome, modalFlags.groupEdit]);

  useEffect(() => {
    if (!modalFlags.addActor && !editingActor) return;
    void ensureRuntimesLoaded();
  }, [editingActor, ensureRuntimesLoaded, modalFlags.addActor]);

  // ============ Actions ============

  // ============ Computed for ChatTab ============

  // ============ Render ============

  return (
    <div
      className={`w-full relative overflow-hidden ${isDark
          ? "bg-black text-slate-100"
          : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
        }`}
      style={{ height: "calc(100% - var(--vk-offset, 0px))" }}
    >
      {/* Background orbs — hidden on mobile for GPU performance */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden hidden md:block">
        <div
          className={`absolute -top-32 -left-32 w-96 h-96 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-br from-cyan-500/10 via-cyan-600/5 to-transparent"
              : "bg-gradient-to-br from-cyan-400/15 via-cyan-500/5 to-transparent"
            }`}
          style={{ opacity: 0.75 }}
        />
        <div
          className={`absolute top-1/4 -right-24 w-80 h-80 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-bl from-purple-500/10 via-indigo-600/5 to-transparent"
              : "bg-gradient-to-bl from-purple-400/10 via-indigo-500/5 to-transparent"
            }`}
          style={{ animationDelay: "-3s", opacity: 0.65 }}
        />
        <div
          className={`absolute -bottom-20 left-1/3 w-72 h-72 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-tr from-blue-500/10 via-sky-600/5 to-transparent"
              : "bg-gradient-to-tr from-blue-400/10 via-sky-500/5 to-transparent"
            }`}
          style={{ animationDelay: "-5s", opacity: 0.6 }}
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
          onWarmGroup={(gid) => void warmGroup(gid)}
          onCreateGroup={
            !webReadOnly && canManageGroups
              ? () => {
                openModal("createGroup");
                void fetchDirSuggestions();
              }
              : undefined
          }
          onClose={() => setSidebarOpen(false)}
          onToggleCollapse={toggleSidebarCollapsed}
          onReorder={reorderGroups}
        />

        {/* Main content */}
        <main
          className={`absolute inset-0 md:relative md:inset-auto h-full flex flex-col overflow-hidden ${isDark ? "bg-black/75" : "bg-white/80"
            }`}
        >
          <AppHeader
            isDark={isDark}
            theme={theme}
            onThemeChange={setTheme}
            webReadOnly={webReadOnly}
            webVersion={__WEB_VERSION__}
            selectedGroupId={selectedGroupId}
            groupDoc={groupDoc}
            selectedGroupRunning={selectedGroupRunning}
            actors={actors}
            sseStatus={sseStatus}
            daemonVersion={daemonVersion}
            busy={busy}
            onOpenSidebar={() => setSidebarOpen(true)}
            onOpenGroupEdit={canManageGroups ? () => {
              if (groupDoc) {
                setEditGroupTitle(groupDoc.title || "");
                setEditGroupTopic(groupDoc.topic || "");
                openModal("groupEdit");
              }
            } : undefined}
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
                const agentState =
                  (groupContext?.agent_states || []).find((p) => p.id === (actor?.id || "")) || null;

                return (
                  <div key={actorId} className={isVisible ? "flex min-h-0 flex-col flex-1" : "hidden"}>
                    <ErrorBoundary>
                    <ActorTab
                      actor={actor}
                      groupId={selectedGroupId}
                      agentState={agentState}
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

      {!webReadOnly && (errorMsg || notice) ? (
        <div className="pointer-events-none fixed inset-x-0 top-4 z-[1200] flex flex-col items-center gap-3 px-4">
          {errorMsg ? (
            <div
              className={classNames(
                "pointer-events-auto flex w-full max-w-xl items-start gap-3 rounded-2xl px-4 py-3 text-sm shadow-2xl glass-modal animate-slide-up",
                isDark ? "border-rose-500/20 text-rose-300" : "border-rose-200/50 text-rose-700"
              )}
              role="alert"
            >
              <span className="min-w-0 flex-1 break-words">{errorMsg}</span>
              <button
                type="button"
                className={classNames(
                  "flex min-h-[36px] min-w-[36px] items-center justify-center rounded-lg p-2 transition-all glass-btn",
                  isDark ? "text-rose-400" : "text-rose-600"
                )}
                onClick={dismissError}
                aria-label={t("layout:dismissError")}
              >
                ×
              </button>
            </div>
          ) : null}

          {notice ? (
            <div
              className={classNames(
                "pointer-events-auto flex w-full max-w-xl items-start gap-3 rounded-2xl px-4 py-3 text-sm shadow-2xl glass-modal animate-slide-up",
                isDark ? "border-white/10 text-slate-200" : "border-black/10 text-gray-800"
              )}
              role="status"
            >
              <span className="min-w-0 flex-1 break-words">{notice.message}</span>
              {notice.actionId && notice.actionLabel ? (
                <button
                  type="button"
                  className={classNames(
                    "rounded-xl px-2 py-1 text-xs transition-all glass-btn",
                    isDark ? "text-slate-100" : "text-gray-900"
                  )}
                  onClick={dismissNotice}
                >
                  {notice.actionLabel}
                </button>
              ) : null}
              <button
                type="button"
                className={classNames(
                  "flex min-h-[36px] min-w-[36px] items-center justify-center rounded-lg p-2 transition-all glass-btn",
                  isDark ? "text-slate-300" : "text-gray-600"
                )}
                onClick={dismissNotice}
                aria-label={t("common:dismiss")}
              >
                ×
              </button>
            </div>
          ) : null}
        </div>
      ) : null}

      {/* Modals (lazy-loaded — not needed on first paint) */}
      <Suspense fallback={null}>
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
          canManageGroups={canManageGroups}
        />
      </Suspense>

      <DropOverlay isOpen={dropOverlayOpen} isDark={isDark} maxFileMb={WEB_MAX_FILE_MB} />
    </div>
  );
}
