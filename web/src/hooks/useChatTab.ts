// useChatTab - Encapsulates ChatTab business logic and state.
// Reduces prop drilling by providing state from stores and computed values directly.

import { useMemo, useCallback } from "react";
import {
  useGroupStore,
  useUIStore,
  useComposerStore,
  useModalStore,
  useFormStore,
} from "../stores";
import type { Actor, LedgerEvent, ChatMessageData } from "../types";
import * as api from "../services/api";

interface UseChatTabOptions {
  selectedGroupId: string;
  actors: Actor[];
  recipientActors: Actor[];
  /** Callback for when message is sent */
  onMessageSent?: () => void;
  /** Refs for composer interactions */
  composerRef?: React.RefObject<HTMLTextAreaElement>;
  fileInputRef?: React.RefObject<HTMLInputElement>;
  /** Chat at bottom ref for scroll state */
  chatAtBottomRef?: React.MutableRefObject<boolean>;
  /** Scroll memory ref for restoring positions */
  chatScrollMemoryRef?: React.MutableRefObject<
    Record<string, { atBottom: boolean; anchorId: string; offsetPx: number }>
  >;
}

export function useChatTab({
  selectedGroupId,
  actors,
  recipientActors,
  onMessageSent,
  composerRef,
  fileInputRef,
  chatAtBottomRef,
  chatScrollMemoryRef,
}: UseChatTabOptions) {
  // ============ Stores ============
  const {
    events,
    chatWindow,
    groupDoc,
    groupContext,
    groupSettings,
    hasMoreHistory,
    isLoadingHistory,
    isChatWindowLoading,
    closeChatWindow,
    openChatWindow,
    loadMoreHistory,
    updateAckStatus,
  } = useGroupStore();

  const {
    busy,
    chatFilter,
    showScrollButton,
    chatUnreadCount,
    setBusy,
    setChatFilter,
    setShowScrollButton,
    setChatUnreadCount,
    showError,
    showNotice,
  } = useUIStore();

  const {
    composerText,
    composerFiles,
    toText,
    replyTarget,
    priority,
    replyRequired,
    destGroupId,
    setComposerText,
    setComposerFiles,
    setToText,
    setReplyTarget,
    setPriority,
    setReplyRequired,
    setDestGroupId,
    clearComposer,
    clearDraft,
  } = useComposerStore();

  const { setRecipientsModal, setRelayModal, openModal } = useModalStore();
  const { setNewActorRole } = useFormStore();

  // ============ Computed Values ============

  // Valid recipient tokens
  const validRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of recipientActors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [recipientActors]);

  // Parse toText into validated tokens
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

  // Mention suggestions
  const mentionSuggestions = useMemo(() => {
    const base = ["@all", "@foreman", "@peers"];
    const actorIds = recipientActors.map((a) => String(a.id || "")).filter((id) => id);
    return [...base, ...actorIds];
  }, [recipientActors]);

  // Send group ID (respects cross-group destination)
  const sendGroupId = useMemo(() => {
    const raw = String(destGroupId || "").trim();
    return raw || selectedGroupId;
  }, [destGroupId, selectedGroupId]);

  // Project root
  const projectRoot = useMemo(() => {
    if (!groupDoc) return "";
    const key = String(groupDoc.active_scope_key || "");
    if (!key) return "";
    const scopes = Array.isArray(groupDoc.scopes) ? groupDoc.scopes : [];
    const hit = scopes.find((s) => String(s.scope_key || "") === key);
    return String(hit?.url || "");
  }, [groupDoc]);

  // Has foreman
  const hasForeman = useMemo(() => actors.some((a) => a.role === "foreman"), [actors]);

  // Selected group running state
  const selectedGroupRunning = useMemo(() => {
    const anyActorRunning = actors.some((a) => !!a.running);
    return anyActorRunning;
  }, [actors]);

  // Setup checklist conditions
  const needsScope = !!selectedGroupId && !projectRoot;
  const needsActors = !!selectedGroupId && actors.length === 0;
  const needsStart = !!selectedGroupId && actors.length > 0 && !selectedGroupRunning;
  const showSetupCard = needsScope || needsActors || needsStart;

  // In chat window mode
  const inChatWindow = useMemo(() => {
    return !!chatWindow && String(chatWindow.groupId || "") === String(selectedGroupId || "");
  }, [chatWindow, selectedGroupId]);

  // Filtered live chat messages
  const liveChatMessages = useMemo(() => {
    const all = events.filter((ev) => ev.kind === "chat.message");
    if (chatFilter === "attention") {
      return all.filter((ev) => {
        const d = ev.data as ChatMessageData | undefined;
        return String(d?.priority || "normal") === "attention";
      });
    }
    if (chatFilter === "task") {
      return all.filter((ev) => {
        const d = ev.data as ChatMessageData | undefined;
        return !!d?.reply_required;
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

  // Chat messages (window or live)
  const chatMessages = useMemo(() => {
    if (inChatWindow && chatWindow) return chatWindow.events || [];
    return liveChatMessages;
  }, [chatWindow, inChatWindow, liveChatMessages]);

  const hasAnyChatMessages = useMemo(
    () => events.some((ev) => ev.kind === "chat.message"),
    [events]
  );

  // Chat view key for VirtualMessageList
  const chatViewKey = useMemo(() => {
    if (inChatWindow && chatWindow) {
      return `${selectedGroupId}:window:${chatWindow.centerEventId}`;
    }
    return `${selectedGroupId}:live`;
  }, [inChatWindow, chatWindow, selectedGroupId]);

  // Chat window props (for jump-to mode)
  const chatWindowProps = useMemo(() => {
    if (!inChatWindow || !chatWindow) return null;
    return {
      centerEventId: chatWindow.centerEventId,
      hasMoreBefore: chatWindow.hasMoreBefore,
      hasMoreAfter: chatWindow.hasMoreAfter,
    };
  }, [inChatWindow, chatWindow]);

  // Initial scroll target (for window mode)
  const chatInitialScrollTargetId = useMemo(() => {
    if (inChatWindow && chatWindow) return chatWindow.centerEventId;
    return undefined;
  }, [inChatWindow, chatWindow]);

  // Highlight event ID (for window mode)
  const chatHighlightEventId = useMemo(() => {
    if (inChatWindow && chatWindow) return chatWindow.centerEventId;
    return undefined;
  }, [inChatWindow, chatWindow]);

  // Presence agents
  const presenceAgents = useMemo(
    () => groupContext?.presence?.agents || [],
    [groupContext]
  );

  // ============ Actions ============

  const toggleRecipient = useCallback(
    (token: string) => {
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
    },
    [toTokens, setToText]
  );

  const clearRecipients = useCallback(() => setToText(""), [setToText]);

  const appendRecipientToken = useCallback(
    (token: string) => {
      setToText(toText ? toText + ", " + token : token);
    },
    [toText, setToText]
  );

  const removeComposerFile = useCallback(
    (idx: number) => {
      setComposerFiles(composerFiles.filter((_, i) => i !== idx));
    },
    [composerFiles, setComposerFiles]
  );

  const sendMessage = useCallback(async () => {
    const txt = composerText.trim();
    if (!selectedGroupId) return;
    if (!txt && composerFiles.length === 0) return;

    const dstGroup = String(sendGroupId || "").trim();
    const isCrossGroup = !!dstGroup && dstGroup !== selectedGroupId;

    const prio = replyRequired ? "attention" : (priority || "normal");

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
          prio,
          replyRequired
        );
      } else {
        if (isCrossGroup) {
          if (composerFiles.length > 0) {
            showError("Cross-group send does not support attachments yet.");
            return;
          }
          resp = await api.sendCrossGroupMessage(selectedGroupId, dstGroup, txt, to, prio, replyRequired);
        } else {
          resp = await api.sendMessage(
            selectedGroupId,
            txt,
            to,
            composerFiles.length > 0 ? composerFiles : undefined,
            prio,
            replyRequired
          );
        }
      }
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      clearComposer();
      setDestGroupId(selectedGroupId);
      clearDraft(selectedGroupId);
      if (fileInputRef?.current) fileInputRef.current.value = "";
      if (inChatWindow) {
        closeChatWindow();
        const url = new URL(window.location.href);
        url.searchParams.delete("event");
        url.searchParams.delete("tab");
        window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
      }
      onMessageSent?.();
    } finally {
      setBusy("");
    }
  }, [
    composerText,
    composerFiles,
    selectedGroupId,
    sendGroupId,
    priority,
    replyRequired,
    toTokens,
    replyTarget,
    inChatWindow,
    setBusy,
    showError,
    setDestGroupId,
    clearComposer,
    clearDraft,
    closeChatWindow,
    fileInputRef,
    onMessageSent,
  ]);

  const acknowledgeMessage = useCallback(
    async (eventId: string) => {
      const eid = String(eventId || "").trim();
      if (!eid) return;
      if (!selectedGroupId) return;
      const resp = await api.ackMessage(selectedGroupId, eid);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      updateAckStatus(eid, "user");
    },
    [selectedGroupId, showError, updateAckStatus]
  );

  const copyMessageLink = useCallback(
    async (eventId: string) => {
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
    },
    [selectedGroupId, showNotice, showError]
  );

  const startReply = useCallback(
    (ev: LedgerEvent) => {
      if (!ev.id || ev.kind !== "chat.message") return;
      const data = ev.data as ChatMessageData | undefined;
      const text = data?.text ? String(data.text) : "";

      // Reply is always in the current group.
      if (selectedGroupId) {
        setDestGroupId(selectedGroupId);
      }

      // Pre-fill recipients to match daemon default reply routing
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
      requestAnimationFrame(() => composerRef?.current?.focus());
    },
    [selectedGroupId, actors, groupSettings, setDestGroupId, setToText, setReplyTarget, composerRef]
  );

  const cancelReply = useCallback(() => setReplyTarget(null), [setReplyTarget]);

  const showRecipients = useCallback(
    (eventId: string) => setRecipientsModal(eventId),
    [setRecipientsModal]
  );

  const relayMessage = useCallback(
    (ev: LedgerEvent) => setRelayModal(ev.id ?? null, selectedGroupId, ev),
    [setRelayModal, selectedGroupId]
  );

  const openSourceMessage = useCallback(
    (srcGroupId: string, srcEventId: string) => {
      const gid = String(srcGroupId || "").trim();
      const eid = String(srcEventId || "").trim();
      if (!gid || !eid) return;

      const url = new URL(window.location.href);
      url.searchParams.set("group", gid);
      url.searchParams.set("event", eid);
      url.searchParams.set("tab", "chat");
      window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());

      if (selectedGroupId === gid) {
        useUIStore.getState().setActiveTab("chat");
        void openChatWindow(gid, eid);
      } else {
        // Queue deep link and switch groups
        useGroupStore.getState().setSelectedGroupId(gid);
        // Note: App.tsx handles the deep link effect
      }
    },
    [selectedGroupId, openChatWindow]
  );

  const exitChatWindow = useCallback(() => {
    closeChatWindow();
    const url = new URL(window.location.href);
    url.searchParams.delete("event");
    url.searchParams.delete("tab");
    window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
  }, [closeChatWindow]);

  const handleScrollButtonClick = useCallback(() => {
    if (chatAtBottomRef) chatAtBottomRef.current = true;
    setShowScrollButton(false);
    setChatUnreadCount(0);
    if (selectedGroupId && chatScrollMemoryRef) {
      chatScrollMemoryRef.current[selectedGroupId] = { atBottom: true, anchorId: "", offsetPx: 0 };
    }
  }, [selectedGroupId, chatAtBottomRef, chatScrollMemoryRef, setShowScrollButton, setChatUnreadCount]);

  const handleScrollChange = useCallback(
    (isAtBottom: boolean) => {
      if (chatAtBottomRef) chatAtBottomRef.current = isAtBottom;
      setShowScrollButton(!isAtBottom);
      if (isAtBottom) setChatUnreadCount(0);
    },
    [chatAtBottomRef, setShowScrollButton, setChatUnreadCount]
  );

  const handleScrollSnapshot = useCallback(
    (snap: { atBottom: boolean; anchorId: string; offsetPx: number }, overrideGroupId?: string) => {
      if (inChatWindow && !overrideGroupId) return;
      const gid = String(overrideGroupId || selectedGroupId || "").trim();
      if (!gid || !chatScrollMemoryRef) return;
      chatScrollMemoryRef.current[gid] = snap;
    },
    [inChatWindow, selectedGroupId, chatScrollMemoryRef]
  );

  const addAgent = useCallback(() => {
    setNewActorRole(hasForeman ? "peer" : "foreman");
    openModal("addActor");
  }, [hasForeman, openModal, setNewActorRole]);

  // ============ Return ============

  return {
    // Chat state
    chatMessages,
    hasAnyChatMessages,
    chatFilter,
    setChatFilter,
    chatViewKey,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatHighlightEventId,
    inChatWindow,
    isLoadingHistory: inChatWindow ? isChatWindowLoading : isLoadingHistory,
    hasMoreHistory: inChatWindow ? false : hasMoreHistory,
    loadMoreHistory: inChatWindow ? undefined : loadMoreHistory,

    // UI state
    busy,
    showScrollButton,
    chatUnreadCount,

    // Setup checklist
    showSetupCard,
    needsScope,
    needsActors,
    needsStart,
    hasForeman,

    // Composer state
    composerText,
    setComposerText,
    composerFiles,
    setComposerFiles,
    removeComposerFile,
    replyTarget,
    cancelReply,
    toTokens,
    toggleRecipient,
    clearRecipients,
    appendRecipientToken,
    priority,
    replyRequired,
    setPriority,
    setReplyRequired,
    destGroupId: sendGroupId,
    setDestGroupId,
    mentionSuggestions,

    // Presence
    presenceAgents,

    // Actions
    sendMessage,
    acknowledgeMessage,
    copyMessageLink,
    startReply,
    showRecipients,
    relayMessage,
    openSourceMessage,
    exitChatWindow,
    handleScrollButtonClick,
    handleScrollChange,
    handleScrollSnapshot,
    addAgent,
  };
}
