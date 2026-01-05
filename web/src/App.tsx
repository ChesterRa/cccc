import React, { useEffect, useMemo, useRef } from "react";
import { TabBar } from "./components/TabBar";
import { DropOverlay } from "./components/DropOverlay";
import { AppModals } from "./components/AppModals";
import { AppHeader } from "./components/layout/AppHeader";
import { GroupSidebar } from "./components/layout/GroupSidebar";
import { useTheme } from "./hooks/useTheme";
import { useActorActions } from "./hooks/useActorActions";
import { useSSE, getRecipientActorIdsForEvent } from "./hooks/useSSE";
import { useDragDrop } from "./hooks/useDragDrop";
import { useGroupActions } from "./hooks/useGroupActions";
import { useSwipeNavigation } from "./hooks/useSwipeNavigation";
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
import type { GroupDoc, LedgerEvent, Actor, ChatMessageData } from "./types";

// Helper function
function getProjectRoot(group: GroupDoc | null): string {
  if (!group) return "";
  const key = String(group.active_scope_key || "");
  if (!key) return "";
  const scopes = Array.isArray(group.scopes) ? group.scopes : [];
  const hit = scopes.find((s) => String(s.scope_key || "") === key);
  return String(hit?.url || "");
}

// ============ Main App Component ============

export default function App() {
  // Theme
  const { theme, setTheme, isDark } = useTheme();

  // Zustand stores
  const {
    groups,
    selectedGroupId,
    groupDoc,
    events,
    actors,
    setSelectedGroupId,
    refreshGroups,
    refreshActors,
    loadGroup,
  } = useGroupStore();

  const {
    busy,
    errorMsg,
    isTransitioning,
    sidebarOpen,
    activeTab,
    showScrollButton,
    chatUnreadCount,
    isSmallScreen,
    setBusy,
    showError,
    dismissError,
    setSidebarOpen,
    setActiveTab,
    setShowScrollButton,
    setChatUnreadCount,
    setSmallScreen,
  } = useUIStore();

  const { recipientsEventId, openModal, setRecipientsModal } = useModalStore();

  const {
    composerText,
    composerFiles,
    toText,
    replyTarget,
    setComposerText,
    setComposerFiles,
    setToText,
    setReplyTarget,
    clearComposer,
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
  // Local state
  const [chatScrollTop, setChatScrollTop] = React.useState(0); // 保存 chat 滚动位置
  const [showMentionMenu, setShowMentionMenu] = React.useState(false);
  const [mentionFilter, setMentionFilter] = React.useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = React.useState(0);

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

  // Tab list for swipe navigation
  const allTabs = useMemo(() => {
    return ["chat", ...actors.map((a) => a.id)];
  }, [actors]);

  // 切换 tab 时先保存滚动位置（在组件卸载前同步执行）
  const handleTabChange = React.useCallback((newTab: string) => {
    if (activeTab === "chat" && newTab !== "chat") {
      const container = eventContainerRef.current;
      if (container) {
        setChatScrollTop(container.scrollTop);
      }
    }
    setActiveTab(newTab);
  }, [activeTab, setActiveTab]);

  // Swipe navigation
  const { handleTouchStart, handleTouchEnd } = useSwipeNavigation({
    tabs: allTabs,
    activeTab,
    onTabChange: handleTabChange,
  });

  // Keep refs in sync
  useEffect(() => {
    activeTabRef.current = activeTab;
    // 进入 chat tab 时清理未读计数并滚动到底部
    if (activeTab === "chat") {
      setChatUnreadCount(0);
      setShowScrollButton(false);
      // 延迟滚动，等待 hidden 类移除后虚拟列表重新计算
      const timer = setTimeout(() => {
        const container = eventContainerRef.current;
        if (container) {
          container.scrollTo({ top: container.scrollHeight, behavior: "auto" });
        }
        chatAtBottomRef.current = true;
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [activeTab, setChatUnreadCount, setShowScrollButton]);

  useEffect(() => {
    actorsRef.current = actors;
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
  const projectRoot = useMemo(() => getProjectRoot(groupDoc), [groupDoc]);
  const hasForeman = useMemo(() => actors.some((a) => a.role === "foreman"), [actors]);
  const selectedGroupMeta = useMemo(
    () => groups.find((g) => String(g.group_id || "") === selectedGroupId) || null,
    [groups, selectedGroupId]
  );
  const selectedGroupRunning = selectedGroupMeta?.running ?? false;

  const validRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of actors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [actors]);

  const toTokens = useMemo(() => {
    const raw = toText
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t.length > 0);
    const filtered = raw.filter((t) => t !== "user" && t !== "@user" && t !== "@");
    const out: string[] = [];
    const seen = new Set<string>();
    for (const t of filtered) {
      if (!validRecipientSet.has(t)) continue;
      if (seen.has(t)) continue;
      seen.add(t);
      out.push(t);
    }
    return out;
  }, [toText, validRecipientSet]);

  const mentionSuggestions = useMemo(() => {
    const base = ["@all", "@foreman", "@peers"];
    const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
    const all = [...base, ...actorIds];
    if (!mentionFilter) return all;
    const lower = mentionFilter.toLowerCase();
    return all.filter((s) => s.toLowerCase().includes(lower));
  }, [actors, mentionFilter]);

  const currentActor = useMemo(() => {
    if (activeTab === "chat") return null;
    return actors.find((a) => a.id === activeTab) || null;
  }, [activeTab, actors]);

  // ============ Group Selection Effect ============
  // 只在 selectedGroupId 变化时重新连接，其他函数是稳定引用
  useEffect(() => {
    clearComposer();
    if (fileInputRef.current) fileInputRef.current.value = "";
    resetDragDrop();

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
  // 只在 mount 时执行一次
  useEffect(() => {
    refreshGroups();
    void fetchRuntimes();
    void fetchDirSuggestions();
    const t = window.setInterval(refreshGroups, 5000);
    return () => window.clearInterval(t);
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

  function toggleRecipient(token: string) {
    const t = token.trim();
    if (!t) return;
    const cur = toTokens;
    const idx = cur.findIndex((x) => x === t);
    if (idx >= 0) {
      const next = cur.slice(0, idx).concat(cur.slice(idx + 1));
      setToText(next.join(", "));
    } else {
      setToText(cur.concat([t]).join(", "));
    }
  }

  async function sendMessage() {
    const txt = composerText.trim();
    if (!selectedGroupId) return;
    if (!txt && composerFiles.length === 0) return;
    setBusy("send");
    try {
      const to = toTokens;
      let resp;
      if (replyTarget) {
        const replyBy = String(replyTarget.by || "").trim();
        const replyFallbackTo =
          replyBy && replyBy !== "user" && replyBy !== "unknown" ? [replyBy] : ["@all"];
        const replyTo = to.length ? to : replyFallbackTo;
        resp = await api.replyMessage(
          selectedGroupId,
          txt,
          replyTo,
          replyTarget.eventId,
          composerFiles.length > 0 ? composerFiles : undefined
        );
      } else {
        resp = await api.sendMessage(
          selectedGroupId,
          txt,
          to,
          composerFiles.length > 0 ? composerFiles : undefined
        );
      }
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      clearComposer();
      if (fileInputRef.current) fileInputRef.current.value = "";
    } finally {
      setBusy("");
    }
  }

  function startReply(ev: LedgerEvent) {
    if (!ev.id || ev.kind !== "chat.message") return;
    const data = ev.data as ChatMessageData | undefined;
    const text = data?.text ? String(data.text) : "";
    setReplyTarget({
      eventId: String(ev.id),
      by: String(ev.by || "unknown"),
      text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
    });
  }

  const scrollToBottom = () => {
    const container = eventContainerRef.current;
    if (container) {
      container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
    }
    chatAtBottomRef.current = true;
    setShowScrollButton(false);
    setChatUnreadCount(0);
  };

  // ============ Computed for Modals ============

  const messageMetaEvent = useMemo(() => {
    if (!recipientsEventId) return null;
    return (
      events.find(
        (x) => x.kind === "chat.message" && String(x.id || "") === recipientsEventId
      ) || null
    );
  }, [events, recipientsEventId]);

  const messageMeta = useMemo(() => {
    if (!messageMetaEvent) return null;
    // 类型守卫：检查 data 是否包含 to 数组
    const data = messageMetaEvent.data as { to?: unknown[] } | undefined;
    const toRaw = data && Array.isArray(data.to) ? data.to : [];
    const toTokensList = toRaw
      .map((x) => String(x || "").trim())
      .filter((s) => s.length > 0);
    const toLabel = toTokensList.length > 0 ? toTokensList.join(", ") : "@all";

    const rs =
      messageMetaEvent._read_status && typeof messageMetaEvent._read_status === "object"
        ? messageMetaEvent._read_status
        : null;
    const recipientIds = rs
      ? Object.keys(rs)
      : getRecipientActorIdsForEvent(messageMetaEvent, actors);
    const recipientIdSet = new Set(recipientIds);
    const entries = actors
      .map((a) => String(a.id || ""))
      .filter((id) => id && recipientIdSet.has(id))
      .map((id) => [id, !!(rs && rs[id])] as const);

    return { toLabel, entries };
  }, [actors, messageMetaEvent]);

  const chatMessages = events.filter((ev) => ev.kind === "chat.message");
  const needsScope = !!selectedGroupId && !projectRoot;
  const needsActors = !!selectedGroupId && actors.length === 0;
  const needsStart = !!selectedGroupId && actors.length > 0 && !selectedGroupRunning;
  const showSetupCard = needsScope || needsActors || needsStart;

  // ============ Render ============

  return (
    <div
      className={`h-full w-full relative overflow-hidden ${
        isDark
          ? "bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950"
          : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
      }`}
    >
      {/* Background orbs */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div
          className={`absolute -top-32 -left-32 w-96 h-96 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-br from-cyan-500/20 via-cyan-600/10 to-transparent"
              : "bg-gradient-to-br from-cyan-400/25 via-cyan-500/15 to-transparent"
          }`}
          style={{ filter: "blur(60px)" }}
        />
        <div
          className={`absolute top-1/4 -right-24 w-80 h-80 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-bl from-purple-500/15 via-indigo-600/10 to-transparent"
              : "bg-gradient-to-bl from-purple-400/20 via-indigo-500/10 to-transparent"
          }`}
          style={{ filter: "blur(50px)", animationDelay: "-3s" }}
        />
        <div
          className={`absolute -bottom-20 left-1/3 w-72 h-72 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-tr from-blue-500/12 via-sky-600/8 to-transparent"
              : "bg-gradient-to-tr from-blue-400/15 via-sky-500/10 to-transparent"
          }`}
          style={{ filter: "blur(45px)", animationDelay: "-5s" }}
        />
      </div>

      {/* Noise texture */}
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.015]"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E")`,
        }}
      />

      <div className="relative h-full grid grid-cols-1 md:grid-cols-[280px_1fr] transition-all duration-300">
        <GroupSidebar
          groups={groups}
          selectedGroupId={selectedGroupId}
          isOpen={sidebarOpen}
          isDark={isDark}
          onSelectGroup={(gid) => setSelectedGroupId(gid)}
          onCreateGroup={() => {
            openModal("createGroup");
            void fetchDirSuggestions();
          }}
          onClose={() => setSidebarOpen(false)}
        />

        {/* Main content */}
        <main
          className={`h-full flex flex-col overflow-hidden backdrop-blur-sm ${
            isDark ? "bg-slate-950/40" : "bg-white/60"
          }`}
        >
          <AppHeader
            isDark={isDark}
            theme={theme}
            onThemeChange={setTheme}
            selectedGroupId={selectedGroupId}
            groupDoc={groupDoc}
            selectedGroupRunning={selectedGroupRunning}
            actors={actors}
            busy={busy}
            errorMsg={errorMsg}
            onDismissError={dismissError}
            onOpenSidebar={() => setSidebarOpen(true)}
            onOpenGroupEdit={() => {
              if (groupDoc) {
                setEditGroupTitle(groupDoc.title || "");
                setEditGroupTopic(groupDoc.topic || "");
                openModal("groupEdit");
              }
            }}
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
              onAddAgent={() => {
                setNewActorRole(hasForeman ? "peer" : "foreman");
                openModal("addActor");
              }}
              canAddAgent={!!selectedGroupId}
            />
          )}

          {/* Tab Content - 使用 CSS 隐藏而非条件渲染，避免重新挂载导致的闪烁 */}
          <div
            ref={contentRef}
            className={`flex-1 min-h-0 flex flex-col overflow-hidden transition-opacity duration-150 ${
              isTransitioning ? "opacity-0" : "opacity-100"
            }`}
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
          >
            {/* Chat Tab - 始终挂载，通过 CSS 控制显示 */}
            <div className={`flex-1 min-h-0 flex flex-col ${activeTab !== "chat" ? "hidden" : ""}`}>
              <ChatTab
                isDark={isDark}
                isSmallScreen={isSmallScreen}
                selectedGroupId={selectedGroupId}
                actors={actors}
                busy={busy}
                showSetupCard={showSetupCard}
                needsScope={needsScope}
                needsActors={needsActors}
                needsStart={needsStart}
                hasForeman={hasForeman}
                onAddAgent={() => {
                  setNewActorRole(hasForeman ? "peer" : "foreman");
                  openModal("addActor");
                }}
                onStartGroup={handleStartGroup}
                chatMessages={chatMessages}
                scrollRef={eventContainerRef}
                showScrollButton={showScrollButton}
                chatUnreadCount={chatUnreadCount}
                onScrollButtonClick={scrollToBottom}
                onScrollChange={(isAtBottom) => {
                  chatAtBottomRef.current = isAtBottom;
                  setShowScrollButton(!isAtBottom);
                  if (isAtBottom) setChatUnreadCount(0);
                }}
                onReply={startReply}
                onShowRecipients={(eventId) => setRecipientsModal(eventId)}
                initialScrollTop={chatScrollTop}
                replyTarget={replyTarget}
                onCancelReply={() => setReplyTarget(null)}
                toTokens={toTokens}
                onToggleRecipient={toggleRecipient}
                onClearRecipients={() => setToText("")}
                composerFiles={composerFiles}
                onRemoveComposerFile={(idx) =>
                  setComposerFiles(composerFiles.filter((_, i) => i !== idx))
                }
                appendComposerFiles={handleAppendComposerFiles}
                fileInputRef={fileInputRef}
                composerRef={composerRef}
                composerText={composerText}
                setComposerText={setComposerText}
                onSendMessage={sendMessage}
                showMentionMenu={showMentionMenu}
                setShowMentionMenu={setShowMentionMenu}
                mentionSuggestions={mentionSuggestions}
                mentionSelectedIndex={mentionSelectedIndex}
                setMentionSelectedIndex={setMentionSelectedIndex}
                setMentionFilter={setMentionFilter}
                onAppendRecipientToken={(token) =>
                  setToText(toText ? toText + ", " + token : token)
                }
              />
            </div>
            {/* Agent Tab - 仅在选中 agent 时显示 */}
            {activeTab !== "chat" && (
              <ActorTab
                actor={currentActor}
                groupId={selectedGroupId}
                termEpoch={currentActor ? getTermEpoch(currentActor.id) : 0}
                busy={busy}
                isDark={isDark}
                onToggleEnabled={() => currentActor && toggleActorEnabled(currentActor)}
                onRelaunch={() => currentActor && relaunchActor(currentActor)}
                onEdit={() => currentActor && editActor(currentActor)}
                onRemove={() => currentActor && removeActor(currentActor, activeTab)}
                onInbox={() => currentActor && openActorInbox(currentActor)}
                onStatusChange={() => void refreshActors()}
              />
            )}
          </div>
        </main>
      </div>

      {/* Modals */}
      <AppModals
        isDark={isDark}
        composerRef={composerRef}
        messageMeta={messageMeta}
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
