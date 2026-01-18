import React, { useEffect, useMemo, useRef } from "react";
import { TabBar } from "./components/TabBar";
import { DropOverlay } from "./components/DropOverlay";
import { AppModals } from "./components/AppModals";
import { AppHeader } from "./components/layout/AppHeader";
import { GroupSidebar } from "./components/layout/GroupSidebar";
import { useTheme } from "./hooks/useTheme";
import { useActorActions } from "./hooks/useActorActions";
import { useSSE, getAckRecipientIdsForEvent, getRecipientActorIdsForEvent } from "./hooks/useSSE";
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
  useObservabilityStore,
} from "./stores";
import { handlePwaNoticeAction } from "./pwa";
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
    groupOrder,
    selectedGroupId,
    groupDoc,
    events,
    chatWindow,
    actors,
    groupContext,
    groupSettings,
    hasMoreHistory,
    isLoadingHistory,
    isChatWindowLoading,
    setSelectedGroupId,
    refreshGroups,
    refreshActors,
    loadGroup,
    loadMoreHistory,
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
    showScrollButton,
    chatUnreadCount,
    isSmallScreen,
    chatFilter,
    webReadOnly,
    setBusy,
    showError,
    dismissError,
    dismissNotice,
    showNotice,
    setSidebarOpen,
    toggleSidebarCollapsed,
    setActiveTab,
    setShowScrollButton,
    setChatUnreadCount,
    setSmallScreen,
    setChatFilter,
    setWebReadOnly,
  } = useUIStore();

  const { recipientsEventId, openModal, setRecipientsModal, setRelayModal } = useModalStore();

  const {
    composerText,
    composerFiles,
    toText,
    replyTarget,
    priority,
    destGroupId,
    setComposerText,
    setComposerFiles,
    setToText,
    setReplyTarget,
    setPriority,
    setDestGroupId,
    clearComposer,
    switchGroup,
    clearDraft,
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
  const deepLinkRef = useRef<{ groupId: string; eventId: string } | null>(null);
  // Local state
  const [showMentionMenu, setShowMentionMenu] = React.useState(false);
  const [mentionFilter, setMentionFilter] = React.useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = React.useState(0);
  const [mountedActorIds, setMountedActorIds] = React.useState<string[]>([]);
  const [recipientActors, setRecipientActors] = React.useState<Actor[]>([]);
  const [recipientActorsBusy, setRecipientActorsBusy] = React.useState(false);
  const recipientActorsCacheRef = React.useRef<Record<string, Actor[]>>({});
  const [destGroupScopeLabel, setDestGroupScopeLabel] = React.useState("");
  const groupDocCacheRef = React.useRef<Record<string, GroupDoc>>({});

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
  const projectRoot = useMemo(() => getProjectRoot(groupDoc), [groupDoc]);
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
  const sendGroupId = useMemo(() => {
    const raw = String(destGroupId || "").trim();
    return raw || selectedGroupId;
  }, [destGroupId, selectedGroupId]);

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

  const validRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of recipientActors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [recipientActors]);

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
    const actorIds = recipientActors.map((a) => String(a.id || "")).filter((id) => id);
    const all = [...base, ...actorIds];
    if (!mentionFilter) return all;
    const lower = mentionFilter.toLowerCase();
    return all.filter((s) => s.toLowerCase().includes(lower));
  }, [recipientActors, mentionFilter]);

  React.useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (!destGroupId) {
      setDestGroupId(gid);
    }
  }, [destGroupId, selectedGroupId, setDestGroupId]);

  React.useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    if (replyTarget || composerFiles.length > 0) {
      if (sendGroupId && sendGroupId !== gid) {
        setDestGroupId(gid);
      }
    }
  }, [composerFiles.length, replyTarget, selectedGroupId, sendGroupId, setDestGroupId]);

  React.useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    recipientActorsCacheRef.current[gid] = actors;
  }, [actors, selectedGroupId]);

  React.useEffect(() => {
    const activeScopeLabel = (doc: GroupDoc | null) => {
      if (!doc) return "";
      const key = String(doc.active_scope_key || "").trim();
      if (!key) return "";
      const scopes = Array.isArray(doc.scopes) ? doc.scopes : [];
      const hit = scopes.find((s) => String(s?.scope_key || "").trim() === key);
      const label = String(hit?.label || "").trim();
      const url = String(hit?.url || "").trim();
      return label || url;
    };

    const gid = String(sendGroupId || "").trim();
    if (!gid) {
      setDestGroupScopeLabel("");
      return;
    }

    if (gid === String(selectedGroupId || "").trim()) {
      setDestGroupScopeLabel(activeScopeLabel(groupDoc));
      if (groupDoc) groupDocCacheRef.current[gid] = groupDoc;
      return;
    }

    const cached = groupDocCacheRef.current[gid];
    if (cached) {
      setDestGroupScopeLabel(activeScopeLabel(cached));
      return;
    }

    let cancelled = false;
    setDestGroupScopeLabel("");
    void api.fetchGroup(gid).then((resp) => {
      if (cancelled) return;
      if (!resp.ok) {
        setDestGroupScopeLabel("");
        return;
      }
      const doc = resp.result.group;
      groupDocCacheRef.current[gid] = doc;
      setDestGroupScopeLabel(activeScopeLabel(doc));
    });

    return () => {
      cancelled = true;
    };
  }, [groupDoc, selectedGroupId, sendGroupId]);

  React.useEffect(() => {
    const gid = String(sendGroupId || "").trim();
    if (!gid) {
      setRecipientActors([]);
      setRecipientActorsBusy(false);
      return;
    }
    if (gid === String(selectedGroupId || "").trim()) {
      setRecipientActors(actors);
      setRecipientActorsBusy(false);
      return;
    }
    const cached = recipientActorsCacheRef.current[gid];
    if (cached) {
      setRecipientActors(cached);
      setRecipientActorsBusy(false);
      return;
    }

    let cancelled = false;
    setRecipientActorsBusy(true);
    setRecipientActors([]);
    void api
      .fetchActors(gid)
      .then((resp) => {
        if (cancelled) return;
        if (!resp.ok) {
          setRecipientActors([]);
          return;
        }
        const next = resp.result.actors || [];
        recipientActorsCacheRef.current[gid] = next;
        setRecipientActors(next);
      })
      .finally(() => {
        if (cancelled) return;
        setRecipientActorsBusy(false);
      });

    return () => {
      cancelled = true;
    };
  }, [actors, selectedGroupId, sendGroupId]);

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
    // Capture deep links (?group=<id>&event=<event_id>) on initial load.
    const params = new URLSearchParams(window.location.search);
    const gid = String(params.get("group") || "").trim();
    const eid = String(params.get("event") || "").trim();
    if (gid && eid) {
      deepLinkRef.current = { groupId: gid, eventId: eid };
    }

    refreshGroups();
    void fetchRuntimes();
    void fetchDirSuggestions();
    void useObservabilityStore.getState().load();
    void api.fetchPing().then((resp) => {
      if (resp.ok) {
        setWebReadOnly(Boolean(resp.result?.web?.read_only));
      }
    }).catch(() => {
      /* ignore */
    });

    // Subscribe to global events stream (replaces polling)
    let es: EventSource | null = null;
    let fallbackTimer: number | null = null;
    let errorCount = 0;

    function connectSSE() {
      es = new EventSource(api.withAuthToken("/api/v1/events/stream"));
      es.addEventListener("event", (e) => {
        try {
          const ev = JSON.parse((e as MessageEvent).data || "{}");
          const kind = typeof ev?.kind === "string" ? ev.kind : "";
          if (kind.startsWith("group.")) {
            refreshGroups();
          }
        } catch {
          /* ignore parse errors */
        }
      });
      es.onopen = () => {
        errorCount = 0; // Reset on successful connection
        refreshGroups(); // Re-sync after reconnects (best-effort)
      };
      es.onerror = () => {
        errorCount++;
        // After 3 consecutive errors, fallback to polling
        if (errorCount >= 3 && !fallbackTimer) {
          es?.close();
          es = null;
          fallbackTimer = window.setInterval(refreshGroups, 10000);
        }
      };
    }

    connectSSE();

    return () => {
      es?.close();
      if (fallbackTimer) window.clearInterval(fallbackTimer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Apply deep links after groups are loaded.
  useEffect(() => {
    const dl = deepLinkRef.current;
    if (!dl) return;
    const gid = String(dl.groupId || "").trim();
    const eid = String(dl.eventId || "").trim();
    if (!gid || !eid) {
      deepLinkRef.current = null;
      return;
    }
    const exists = groups.some((g) => String(g.group_id || "") === gid);
    if (!exists) {
      if (groups.length > 0) {
        showError(`Group not found: ${gid}`);
        deepLinkRef.current = null;
      }
      return;
    }
    if (selectedGroupId !== gid) {
      setSelectedGroupId(gid);
      return;
    }

    setActiveTab("chat");
    void openChatWindow(gid, eid);
    deepLinkRef.current = null;
  }, [groups, openChatWindow, selectedGroupId, setActiveTab, setSelectedGroupId, showError]);

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

    const dstGroup = String(sendGroupId || "").trim();
    const isCrossGroup = !!dstGroup && dstGroup !== selectedGroupId;

    const prio = priority || "normal";
    if (prio === "attention") {
      const ok = window.confirm("Send as an IMPORTANT message? Recipients must acknowledge it.");
      if (!ok) return;
    }

    setBusy("send");
    try {
      const to = toTokens;
      let resp;
      if (replyTarget) {
        if (isCrossGroup) {
          showError("Cross-group send does not support replies.");
          setDestGroupId(selectedGroupId);
          return;
        }
        resp = await api.replyMessage(
          selectedGroupId,
          txt,
          to,
          replyTarget.eventId,
          composerFiles.length > 0 ? composerFiles : undefined,
          prio
        );
      } else {
        if (isCrossGroup) {
          if (composerFiles.length > 0) {
            showError("Cross-group send does not support attachments yet.");
            return;
          }
          resp = await api.sendCrossGroupMessage(selectedGroupId, dstGroup, txt, to, prio);
        } else {
          resp = await api.sendMessage(
            selectedGroupId,
            txt,
            to,
            composerFiles.length > 0 ? composerFiles : undefined,
            prio
          );
        }
      }
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      clearComposer();
      setDestGroupId(selectedGroupId);
      clearDraft(selectedGroupId); // Also clear saved draft for this group
      if (fileInputRef.current) fileInputRef.current.value = "";
      if (inChatWindow) {
        closeChatWindow();
        const url = new URL(window.location.href);
        url.searchParams.delete("event");
        url.searchParams.delete("tab");
        window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
      }
    } finally {
      setBusy("");
    }
  }

  async function acknowledgeMessage(eventId: string) {
    const eid = String(eventId || "").trim();
    if (!eid) return;
    if (!selectedGroupId) return;
    const resp = await api.ackMessage(selectedGroupId, eid);
    if (!resp.ok) {
      showError(`${resp.error.code}: ${resp.error.message}`);
      return;
    }
    useGroupStore.getState().updateAckStatus(eid, "user");
  }

  async function copyMessageLink(eventId: string) {
    const eid = String(eventId || "").trim();
    if (!eid || !selectedGroupId) return;

    const url = new URL(window.location.origin + window.location.pathname);
    url.searchParams.set("group", selectedGroupId);
    url.searchParams.set("event", eid);
    url.searchParams.set("tab", "chat");

    const text = url.toString();
    let ok = false;
    try {
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch {
      // Fallback for older browsers / insecure contexts.
      try {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.left = "-9999px";
        ta.style.top = "0";
        document.body.appendChild(ta);
        ta.select();
        ok = document.execCommand("copy");
        document.body.removeChild(ta);
      } catch {
        ok = false;
      }
    }
    if (ok) {
      showNotice({ message: "Link copied" });
    } else {
      showError("Failed to copy link");
    }
  }

  function openMessageWindow(groupId: string, eventId: string) {
    const gid = String(groupId || "").trim();
    const eid = String(eventId || "").trim();
    if (!gid || !eid) return;

    const url = new URL(window.location.href);
    url.searchParams.set("group", gid);
    url.searchParams.set("event", eid);
    url.searchParams.set("tab", "chat");
    window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());

    // If we're already in the target group, jump immediately.
    if (selectedGroupId === gid) {
      setActiveTab("chat");
      void openChatWindow(gid, eid);
      deepLinkRef.current = null;
      return;
    }

    // Otherwise, queue a deep link and switch groups; the deep-link effect will open the window.
    deepLinkRef.current = { groupId: gid, eventId: eid };
    setSelectedGroupId(gid);
  }

  function startReply(ev: LedgerEvent) {
    if (!ev.id || ev.kind !== "chat.message") return;
    const data = ev.data as ChatMessageData | undefined;
    const text = data?.text ? String(data.text) : "";

    // Reply is always in the current group.
    // Also sync recipient chips immediately to avoid a 1-render lag when switching from cross-group send.
    if (selectedGroupId) {
      setDestGroupId(selectedGroupId);
      setRecipientActors(actors);
      setRecipientActorsBusy(false);
    }

    // Pre-fill recipients to match daemon default reply routing:
    // - Replying to someone else -> reply to the author.
    // - Replying to yourself -> preserve the original audience (or fall back to group default policy).
    const by = String(ev.by || "").trim();
    const authorIsActor = by && by !== "user" && actors.some((a) => String(a.id || "") === by);
    const originalTo = Array.isArray(data?.to)
      ? data?.to.map((t) => String(t || "").trim()).filter((t) => t)
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
  }

  const handleScrollButtonClick = () => {
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
    // Type guard: ensure data.to is an array.
    const metaData = messageMetaEvent.data as { to?: unknown[] } | undefined;
    const toRaw = metaData && Array.isArray(metaData.to) ? metaData.to : [];
    const toTokensList = toRaw
      .map((x) => String(x || "").trim())
      .filter((s) => s.length > 0);
    const toLabel = toTokensList.length > 0 ? toTokensList.join(", ") : "@all";

    const msgData = messageMetaEvent.data as ChatMessageData | undefined;
    const isAttention = String(msgData?.priority || "normal") === "attention";

    if (isAttention) {
      const as =
        messageMetaEvent._ack_status && typeof messageMetaEvent._ack_status === "object"
          ? messageMetaEvent._ack_status
          : null;
      const recipientIds = as
        ? Object.keys(as)
        : getAckRecipientIdsForEvent(messageMetaEvent, actors);
      const recipientIdSet = new Set(recipientIds);
      const entries = [
        ...actors
          .map((a) => String(a.id || ""))
          .filter((id) => id && recipientIdSet.has(id))
          .map((id) => [id, !!(as && as[id])] as const),
        recipientIdSet.has("user") ? (["user", !!(as && as["user"])] as const) : null,
      ].filter(Boolean) as Array<readonly [string, boolean]>;

      return { toLabel, entries, statusKind: "ack" as const };
    }

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

    return { toLabel, entries, statusKind: "read" as const };
  }, [actors, messageMetaEvent]);

  const inChatWindow = useMemo(() => {
    return !!chatWindow && String(chatWindow.groupId || "") === String(selectedGroupId || "");
  }, [chatWindow, selectedGroupId]);

  const liveChatMessages = useMemo(() => {
    const all = events.filter((ev) => ev.kind === "chat.message");
    if (chatFilter === "attention") {
      return all.filter((ev) => {
        const d = ev.data as ChatMessageData | undefined;
        return String(d?.priority || "normal") === "attention";
      });
    }
    if (chatFilter === "to_user") {
      return all.filter((ev) => {
        const d = ev.data as ChatMessageData | undefined;
        const dst = typeof d?.dst_group_id === "string" ? String(d.dst_group_id || "").trim() : "";
        if (dst) return false;
        const to = Array.isArray(d?.to) ? d?.to : [];
        return to.includes("user") || to.includes("@user");
      });
    }
    return all;
  }, [events, chatFilter]);

  const chatMessages = useMemo(() => {
    if (inChatWindow && chatWindow) return chatWindow.events || [];
    return liveChatMessages;
  }, [chatWindow, inChatWindow, liveChatMessages]);

  const restoreChatAnchor = useMemo(() => {
    if (!selectedGroupId) return null;
    if (inChatWindow) return null;
    const snap = chatScrollMemoryRef.current[String(selectedGroupId || "").trim()];
    if (!snap || snap.atBottom) return null;
    if (!snap.anchorId) return null;
    return snap;
  }, [inChatWindow, selectedGroupId]);

  const hasAnyChatMessages = useMemo(() => events.some((ev) => ev.kind === "chat.message"), [events]);
  const needsScope = !!selectedGroupId && !projectRoot;
  const needsActors = !!selectedGroupId && actors.length === 0;
  const needsStart = !!selectedGroupId && actors.length > 0 && !selectedGroupRunning;
  const showSetupCard = needsScope || needsActors || needsStart;

  // ============ Render ============

  return (
    <div
      className={`h-full w-full relative overflow-hidden ${isDark
          ? "bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950"
          : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
        }`}
    >
      {/* Background orbs */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div
          className={`absolute -top-32 -left-32 w-96 h-96 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-br from-cyan-500/20 via-cyan-600/10 to-transparent"
              : "bg-gradient-to-br from-cyan-400/25 via-cyan-500/15 to-transparent"
            }`}
          style={{ filter: "blur(60px)" }}
        />
        <div
          className={`absolute top-1/4 -right-24 w-80 h-80 rounded-full liquid-blob ${isDark
              ? "bg-gradient-to-bl from-purple-500/15 via-indigo-600/10 to-transparent"
              : "bg-gradient-to-bl from-purple-400/20 via-indigo-500/10 to-transparent"
            }`}
          style={{ filter: "blur(50px)", animationDelay: "-3s" }}
        />
        <div
          className={`absolute -bottom-20 left-1/3 w-72 h-72 rounded-full liquid-blob ${isDark
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

      <div className={`relative h-full grid grid-cols-1 transition-all duration-300 ${
        sidebarCollapsed ? "md:grid-cols-[60px_1fr]" : "md:grid-cols-[280px_1fr]"
      }`}>
        <GroupSidebar
          orderedGroups={orderedGroups}
          groupOrder={groupOrder}
          selectedGroupId={selectedGroupId}
          isOpen={sidebarOpen}
          isCollapsed={sidebarCollapsed}
          isDark={isDark}
          onSelectGroup={(gid) => setSelectedGroupId(gid)}
          onCreateGroup={() => {
            openModal("createGroup");
            void fetchDirSuggestions();
          }}
          onClose={() => setSidebarOpen(false)}
          onToggleCollapse={toggleSidebarCollapsed}
          onReorder={reorderGroups}
        />

        {/* Main content */}
        <main
          className={`h-full flex flex-col overflow-hidden backdrop-blur-sm ${isDark ? "bg-slate-950/40" : "bg-white/60"
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
            busy={busy}
            errorMsg={errorMsg}
            notice={notice}
            onDismissError={dismissError}
            onNoticeAction={(actionId) => {
              void handlePwaNoticeAction(actionId, {
                showNotice,
                dismissNotice,
              });
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
              onAddAgent={() => {
                setNewActorRole(hasForeman ? "peer" : "foreman");
                openModal("addActor");
              }}
              canAddAgent={!!selectedGroupId}
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
              <ChatTab
                isDark={isDark}
                isSmallScreen={isSmallScreen}
                selectedGroupId={selectedGroupId}
                groupLabelById={groupLabelById}
                actors={actors}
                groups={groups}
                destGroupId={sendGroupId}
                setDestGroupId={setDestGroupId}
                destGroupScopeLabel={destGroupScopeLabel}
                recipientActors={recipientActors}
                recipientActorsBusy={recipientActorsBusy}
                presenceAgents={groupContext?.presence?.agents || []}
                busy={busy}
                chatFilter={chatFilter}
                setChatFilter={setChatFilter}
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
                hasAnyChatMessages={hasAnyChatMessages}
                scrollRef={eventContainerRef}
                showScrollButton={showScrollButton}
                chatUnreadCount={chatUnreadCount}
                onScrollButtonClick={handleScrollButtonClick}
                onScrollChange={(isAtBottom) => {
                  chatAtBottomRef.current = isAtBottom;
                  setShowScrollButton(!isAtBottom);
                  if (isAtBottom) setChatUnreadCount(0);
                }}
                onReply={startReply}
                onShowRecipients={(eventId) => setRecipientsModal(eventId)}
                onAckMessage={acknowledgeMessage}
                onCopyMessageLink={copyMessageLink}
                onRelayMessage={(eventId) => setRelayModal(eventId)}
                onOpenSourceMessage={(srcGroupId, srcEventId) => openMessageWindow(srcGroupId, srcEventId)}
                chatWindow={
                  inChatWindow && chatWindow
                    ? {
                      centerEventId: chatWindow.centerEventId,
                      hasMoreBefore: chatWindow.hasMoreBefore,
                      hasMoreAfter: chatWindow.hasMoreAfter,
                    }
                    : null
                }
                onExitChatWindow={() => {
                  closeChatWindow();
                  const url = new URL(window.location.href);
                  url.searchParams.delete("event");
                  url.searchParams.delete("tab");
                  window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
                }}
                chatViewKey={
                  inChatWindow && chatWindow
                    ? `${selectedGroupId}:window:${chatWindow.centerEventId}`
                    : `${selectedGroupId}:live`
                }
                chatInitialScrollTargetId={inChatWindow && chatWindow ? chatWindow.centerEventId : undefined}
                chatInitialScrollAnchorId={!inChatWindow ? restoreChatAnchor?.anchorId : undefined}
                chatInitialScrollAnchorOffsetPx={!inChatWindow ? restoreChatAnchor?.offsetPx : undefined}
                chatHighlightEventId={inChatWindow && chatWindow ? chatWindow.centerEventId : undefined}
                onScrollSnapshot={(snap) => {
                  if (inChatWindow) return;
                  const gid = String(selectedGroupId || "").trim();
                  if (!gid) return;
                  chatScrollMemoryRef.current[gid] = snap;
                }}
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
                priority={priority}
                setPriority={setPriority}
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
                isLoadingHistory={inChatWindow ? isChatWindowLoading : isLoadingHistory}
                hasMoreHistory={inChatWindow ? false : hasMoreHistory}
                onLoadMore={inChatWindow ? undefined : loadMoreHistory}
              />
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
                    <ActorTab
                      actor={actor}
                      groupId={selectedGroupId}
                      presenceAgent={presence}
                      termEpoch={actor ? getTermEpoch(actor.id) : 0}
                      busy={busy}
                      isDark={isDark}
                      isSmallScreen={isSmallScreen}
                      isVisible={isVisible}
                      onToggleEnabled={() => actor && toggleActorEnabled(actor)}
                      onRelaunch={() => actor && relaunchActor(actor)}
                      onEdit={() => actor && editActor(actor)}
                      onRemove={() => actor && removeActor(actor, activeTab)}
                      onInbox={() => actor && openActorInbox(actor)}
                      onStatusChange={() => void refreshActors()}
                    />
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
