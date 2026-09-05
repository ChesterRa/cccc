import { useMemo, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  useGroupStore,
  useUIStore,
  useComposerStore,
  useModalStore,
  useFormStore,
  selectChatBucketState,
} from "../stores";
import {
  getEffectiveComposerDestGroupId,
  isComposerGroupSettled,
  normalizeReplyMessageMode,
} from "../stores/useComposerStore";
import { getChatSession } from "../stores/useUIStore";
import { useChatOutboxStore, selectOutboxEntries } from "../stores/chatOutboxStore";
import type { Actor, GroupMeta, LedgerEvent, MessageRef } from "../types";
import * as api from "../services/api";
import { formatSendMessageError, shouldBlockLocalCrossGroupAttachments } from "../utils/chatSend";
import { useSlashCommands } from "./useSlashCommands";
import { useSlashSkillDispatch } from "./useSlashSkillDispatch";
import type { ComposerAgentMentionToken, ComposerGroupMentionToken } from "./composerGroupMentions";
import {
  buildComposerGroupBridgeRouteRefs,
  pruneComposerAgentMentionTokens,
  pruneComposerGroupMentionTokens,
} from "./composerGroupMentions";
import { buildComposerLocalGroupRouteRefs } from "./composerLocalGroupRouteRefs";
import { buildComposerSendPlanTargets } from "./composerSendPlan";
import {
  buildComposerMentionSuggestions,
  buildGroupBridgeRouteGroups,
  mergeComposerRouteGroups,
  type ComposerMentionKind,
} from "../pages/chat/chatMentionSuggestions";
import type { GroupBridgeTrust } from "../services/api/groupBridge";
import { subscribeGroupBridgePairingChanged } from "../utils/groupBridgePairingEvents";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "../utils/chatOutboxReconciliation";
import {
  consumeChatSendScrollRequest,
  createChatSendScrollRequest,
  invalidateChatSendScrollRequestForOwner,
  type ChatSendScrollRequest,
} from "../utils/chatSendScrollRequest";

import { buildComposerTrustFetchGroupId } from "./chat/chatTabBasics";
import { shouldFollowChatSendFromViewport } from "./chat/chatSendAutoFollow";
import {
  buildComposerSendRecipientTokens,
  buildComposerSendRoutingSnapshot,
  restoreFailedSendComposerState,
  shouldRestoreComposerAfterFailedSend,
} from "./chat/chatComposerState";
export * from "./chat/chatTabBasics";
export * from "./chat/chatStreamingProjection";
export * from "./chat/chatReplySlots";
export * from "./chat/chatMessageOrdering";
export * from "./chat/chatComposerState";
import {
  buildAssistantPlaceholders,
  buildOptimisticMessage,
  dispatchPreparedMessage,
} from "./chat/chatMessageSend";
import { prepareComposerMessage } from "./chat/prepareComposerMessage";
import { useChatMessageActions } from "./chat/useChatMessageActions";
import { useChatMessageView } from "./chat/useChatMessageView";
import { useTaskReferenceIndex } from "./chat/useTaskReferenceIndex";
import { useShallow } from "zustand/react/shallow";
interface UseChatTabOptions {
  selectedGroupId: string;
  selectedGroupRunning: boolean;
  actors: Actor[];
  recipientActors: Actor[];
  mentionFilter?: string;
  mentionKind?: ComposerMentionKind;
  mentionActorScope?: "selected" | "destination";
  /** Callback for when message is sent */
  onMessageSent?: () => void;
  /** Refs for composer interactions */
  composerRef?: React.RefObject<HTMLTextAreaElement | null>;
  fileInputRef?: React.RefObject<HTMLInputElement | null>;
  /** Chat at bottom ref for scroll state */
  chatAtBottomRef?: React.MutableRefObject<boolean>;
  /** Scroll container ref for programmatic scrolling (e.g. after send) */
  scrollRef?: React.MutableRefObject<HTMLDivElement | null>;
}

export function useChatTab({
  selectedGroupId,
  selectedGroupRunning,
  actors,
  recipientActors,
  mentionFilter = "",
  mentionKind = "agent",
  mentionActorScope = "selected",
  onMessageSent,
  composerRef,
  fileInputRef,
  chatAtBottomRef,
  scrollRef,
}: UseChatTabOptions) {
  const { t } = useTranslation(["chat", "common"]);
  const [sendScrollRequest, setSendScrollRequest] = useState<ChatSendScrollRequest | null>(null);
  const nextSendScrollRequestIdRef = useRef(0);
  const [group_bridgeTrusts, setGroupBridgeTrusts] = useState<GroupBridgeTrust[]>([]);
  const [selectedRemoteGroupIds, setSelectedRemoteGroupIds] = useState<string[]>([]);
  // ============ Stores ============
  const {
    events,
    streamingEvents,
    chatWindow,
    hasMoreHistory,
    hasLoadedTail,
    isLoadingHistory,
    isChatWindowLoading,
  } = useGroupStore(
    useCallback((state) => selectChatBucketState(state, selectedGroupId), [selectedGroupId]),
  );
  const groups = useGroupStore((state) => state.groups);
  const appendEvent = useGroupStore((state) => state.appendEvent);
  const upsertStreamingEvent = useGroupStore((state) => state.upsertStreamingEvent);
  const removeStreamingEventsByPrefix = useGroupStore(
    (state) => state.removeStreamingEventsByPrefix,
  );
  const promoteStreamingEventsByPrefix = useGroupStore(
    (state) => state.promoteStreamingEventsByPrefix,
  );
  const groupDoc = useGroupStore((state) => state.groupDoc);
  const groupContext = useGroupStore((state) => state.groupContext);
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const closeChatWindow = useGroupStore((state) => state.closeChatWindow);
  const openChatWindow = useGroupStore((state) => state.openChatWindow);
  const loadMoreHistory = useGroupStore((state) => state.loadMoreHistory);

  const busy = useUIStore((s) => s.busy);
  const chatSessions = useUIStore((s) => s.chatSessions);
  const setChatFilter = useUIStore((s) => s.setChatFilter);
  const setShowScrollButton = useUIStore((s) => s.setShowScrollButton);
  const setChatUnreadCount = useUIStore((s) => s.setChatUnreadCount);
  const setChatScrollSnapshot = useUIStore((s) => s.setChatScrollSnapshot);
  const setChatMobileSurface = useUIStore((s) => s.setChatMobileSurface);
  const showError = useUIStore((s) => s.showError);

  const setChatAtBottom = useCallback(
    (value: boolean) => {
      if (chatAtBottomRef) chatAtBottomRef.current = value;
    },
    [chatAtBottomRef],
  );
  const showNotice = useUIStore((s) => s.showNotice);

  const chatSession = useMemo(
    () => getChatSession(selectedGroupId, chatSessions),
    [selectedGroupId, chatSessions],
  );
  const { chatFilter, showScrollButton, chatUnreadCount, scrollSnapshot } = chatSession;

  const {
    activeGroupId,
    composerFiles,
    toText,
    replyTarget,
    quotedPresentationRef,
    quotedVoiceDocumentRef,
    messageMode,
    destGroupId,
    setComposerText,
    setComposerFiles,
    setToText,
    setReplyToText,
    setReplyTarget,
    setQuotedPresentationRef,
    setQuotedVoiceDocumentRef,
    setMessageMode,
    setDestGroupId,
    upsertDraft,
    clearDraft,
    clearComposer,
  } = useComposerStore(
    useShallow((s) => ({
      activeGroupId: s.activeGroupId,
      composerFiles: s.composerFiles,
      toText: s.toText,
      replyTarget: s.replyTarget,
      quotedPresentationRef: s.quotedPresentationRef,
      quotedVoiceDocumentRef: s.quotedVoiceDocumentRef,
      messageMode: s.messageMode,
      destGroupId: s.destGroupId,
      setComposerText: s.setComposerText,
      setComposerFiles: s.setComposerFiles,
      setToText: s.setToText,
      setReplyToText: s.setReplyToText,
      setReplyTarget: s.setReplyTarget,
      setQuotedPresentationRef: s.setQuotedPresentationRef,
      setQuotedVoiceDocumentRef: s.setQuotedVoiceDocumentRef,
      setMessageMode: s.setMessageMode,
      setDestGroupId: s.setDestGroupId,
      upsertDraft: s.upsertDraft,
      clearDraft: s.clearDraft,
      clearComposer: s.clearComposer,
    })),
  );
  const composerGroupSettled = isComposerGroupSettled(activeGroupId, selectedGroupId);
  const { setRecipientsModal, setRelayModal, openModal } = useModalStore(
    useShallow((s) => ({
      setRecipientsModal: s.setRecipientsModal,
      setRelayModal: s.setRelayModal,
      openModal: s.openModal,
    })),
  );
  const { setNewActorRole } = useFormStore(
    useShallow((s) => ({ setNewActorRole: s.setNewActorRole })),
  );

  // Outbox (optimistic pending messages) — stable selector, no new array allocation.
  const outboxEntries = useChatOutboxStore(
    useCallback((s) => selectOutboxEntries(s, selectedGroupId), [selectedGroupId]),
  );
  const enqueueOutbox = useChatOutboxStore((s) => s.enqueue);
  const removeOutbox = useChatOutboxStore((s) => s.remove);
  const sendInFlightRef = useRef(false);
  const [composerGroupMentionTokens, setComposerGroupMentionTokens] = useState<
    ComposerGroupMentionToken[]
  >([]);
  const [composerAgentMentionTokens, setComposerAgentMentionTokens] = useState<
    ComposerAgentMentionToken[]
  >([]);

  // ============ Computed Values ============

  const resolveAssistantTargets = useCallback(
    (tokens: string[]): Actor[] => {
      const normalized = tokens.map((token) => String(token || "").trim()).filter((token) => token);
      const resolved = new Map<string, Actor>();
      const policy = groupSettings?.default_send_to || "foreman";
      const effectiveTokens =
        normalized.length > 0 ? normalized : policy === "foreman" ? ["@foreman"] : ["@all"];
      const allActors = actors.filter((actor) => {
        const actorId = String(actor.id || "").trim();
        const internalKind = String(actor.internal_kind || "").trim();
        return actorId && actorId !== "user" && !internalKind;
      });
      const peers = allActors.filter((actor) => String(actor.role || "").trim() !== "foreman");
      const foremen = allActors.filter((actor) => String(actor.role || "").trim() === "foreman");

      const addActors = (items: Actor[]) => {
        for (const actor of items) {
          const actorId = String(actor.id || "").trim();
          if (!actorId || resolved.has(actorId)) continue;
          resolved.set(actorId, actor);
        }
      };

      for (const token of effectiveTokens) {
        if (token === "@all") {
          addActors(allActors);
          continue;
        }
        if (token === "@peers") {
          addActors(peers);
          continue;
        }
        if (token === "@foreman") {
          addActors(foremen);
          continue;
        }
        const actor = allActors.find((item) => String(item.id || "").trim() === token);
        if (actor) addActors([actor]);
      }

      return Array.from(resolved.values()).filter(
        (actor) => String(actor.runtime || "").trim() === "codex",
      );
    },
    [actors, groupSettings?.default_send_to],
  );

  // Recipient parsing accepts tokens selected from either the current group or
  // the destination group, because @ suggestions can switch source by cursor
  // position while selected chips still serialize into one toText field.
  const validRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of actors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    for (const a of recipientActors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [actors, recipientActors]);

  const crossGroupValidRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of recipientActors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [recipientActors]);

  // Send group ID (respects cross-group destination)
  const sendGroupId = useMemo(() => {
    return getEffectiveComposerDestGroupId(destGroupId, activeGroupId, selectedGroupId);
  }, [destGroupId, activeGroupId, selectedGroupId]);

  // Parse toText into validated tokens
  const toTokens = useMemo(() => {
    return buildComposerSendRecipientTokens({
      toText,
      isCrossGroup: !!sendGroupId && !!selectedGroupId && sendGroupId !== selectedGroupId,
      validRecipientSet,
      crossGroupValidRecipientSet,
    });
  }, [crossGroupValidRecipientSet, sendGroupId, selectedGroupId, toText, validRecipientSet]);

  const refreshGroupBridgeTrusts = useCallback(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) {
      setGroupBridgeTrusts([]);
      return;
    }
    let cancelled = false;
    void api.fetchGroupBridgeTrusts(buildComposerTrustFetchGroupId(gid)).then((resp) => {
      if (cancelled) return;
      setGroupBridgeTrusts(resp.ok ? resp.result.trusts || [] : []);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedGroupId]);

  useEffect(() => refreshGroupBridgeTrusts(), [refreshGroupBridgeTrusts]);

  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    return subscribeGroupBridgePairingChanged(gid, refreshGroupBridgeTrusts);
  }, [refreshGroupBridgeTrusts, selectedGroupId]);

  const remoteRouteGroups = useMemo(
    () => buildGroupBridgeRouteGroups(group_bridgeTrusts),
    [group_bridgeTrusts],
  );

  const composerRouteGroups: GroupMeta[] = useMemo(
    () => mergeComposerRouteGroups(groups, remoteRouteGroups),
    [remoteRouteGroups, groups],
  );

  useEffect(() => {
    setSelectedRemoteGroupIds([]);
  }, [selectedGroupId]);

  useEffect(() => {
    const validRemoteIds = new Set(
      remoteRouteGroups.map((group) => String(group.group_id || "").trim()).filter(Boolean),
    );
    setSelectedRemoteGroupIds((current) => {
      const next = current.filter((groupId) => validRemoteIds.has(groupId));
      return next.length === current.length ? current : next;
    });
  }, [remoteRouteGroups]);

  // Message-body mentions are text helpers: @ autocompletes names/references, # adds delegation hints.
  const mentionSuggestions = useMemo(() => {
    const mentionActors =
      mentionKind === "agent" && mentionActorScope === "selected" ? actors : recipientActors;
    return buildComposerMentionSuggestions({
      kind: mentionKind,
      filter: mentionFilter,
      recipientActors: mentionActors,
      groups: composerRouteGroups,
    });
  }, [actors, composerRouteGroups, mentionActorScope, mentionFilter, mentionKind, recipientActors]);

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
  // Setup checklist conditions
  const needsScope = !!selectedGroupId && !projectRoot;
  const needsActors = !!selectedGroupId && actors.length === 0;
  const needsStart = !!selectedGroupId && actors.length > 0 && !selectedGroupRunning;
  const showSetupCard = needsScope || needsActors || needsStart;
  const dispatchSlashSkillMessage = useSlashSkillDispatch({
    selectedGroupId,
    toTokens,
    clearDraft,
    setChatUnreadCount,
    setChatFilter,
    setChatMobileSurface,
    enqueueOutbox,
    removeOutbox,
    showError,
    onMessageSent,
    t,
  });

  const { slashCommands, tryExecuteSlashCommand } = useSlashCommands({
    selectedGroupId,
    clearComposer,
    restoreComposerText: setComposerText,
    showError,
    showNotice,
    dispatchMessage: dispatchSlashSkillMessage,
    onExecuted: () => {
      if (fileInputRef?.current) fileInputRef.current.value = "";
    },
    t,
  });

  const {
    inChatWindow,
    chatViewKey,
    liveWorkEvents,
    unfilteredLiveChatMessages,
    chatMessages,
    hasAnyChatMessages,
    chatInitialScrollAnchorId,
    chatInitialScrollAnchorOffsetPx,
    chatInitialScrollOffsetPx,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatHighlightEventId,
    effectiveIsLoadingHistory,
    effectiveHasMoreHistory,
    chatEmptyState,
  } = useChatMessageView({
    selectedGroupId,
    events,
    streamingEvents,
    outboxEntries,
    chatWindow,
    chatFilter,
    scrollSnapshot,
    hasLoadedTail,
    hasMoreHistory,
    isLoadingHistory,
    isChatWindowLoading,
    groupDoc,
    groupContext,
    groupSettings,
    actors,
    needsActors,
  });

  const shouldFollowCurrentSend = useCallback(
    () => shouldFollowChatSendFromViewport(scrollRef?.current, chatMessages.length),
    [chatMessages.length, scrollRef],
  );

  useEffect(() => {
    setSendScrollRequest((current) =>
      invalidateChatSendScrollRequestForOwner(current, selectedGroupId, chatViewKey),
    );
  }, [chatViewKey, selectedGroupId]);

  const consumeSendScrollRequest = useCallback((requestId: number) => {
    setSendScrollRequest((current) => consumeChatSendScrollRequest(current, requestId));
  }, []);

  const updateChatFilter = useCallback(
    (nextFilter: ReturnType<typeof getChatSession>["chatFilter"]) => {
      if (!selectedGroupId) return;
      setChatFilter(selectedGroupId, nextFilter);
    },
    [selectedGroupId, setChatFilter],
  );

  // Agent state snapshot
  const agentStates = useMemo(() => groupContext?.agent_states || [], [groupContext]);
  const taskById = useTaskReferenceIndex({
    groupId: selectedGroupId,
    events: chatMessages,
    tasksVersion: groupContext?.tasks_version,
    seedTasks: groupContext?.coordination?.tasks,
  });

  // ============ Actions ============

  const toggleRecipient = useCallback(
    (token: string) => {
      const t = token.trim();
      if (!t) return;
      const cur = toTokens;
      const idx = cur.findIndex((x) => x === t);
      if (idx >= 0) {
        const next = cur.slice(0, idx).concat(cur.slice(idx + 1));
        setComposerAgentMentionTokens((tokens) =>
          tokens.filter((mention) => mention.scope !== "selected" || mention.actorId !== t),
        );
        setToText(next.join(", "));
      } else {
        setToText(cur.concat([t]).join(", "));
      }
    },
    [toTokens, setToText],
  );

  const toggleRemoteGroupRecipient = useCallback((groupId: string) => {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    setSelectedRemoteGroupIds((current) => {
      if (current.includes(gid)) return current.filter((item) => item !== gid);
      return [...current, gid];
    });
  }, []);

  const clearRecipients = useCallback(() => {
    setSelectedRemoteGroupIds([]);
    setToText("");
  }, [setToText]);

  const syncMentionRecipientsFromComposerText = useCallback(
    (textOrUpdater: string | ((prev: string) => string)) => {
      // Read the live text instead of subscribing to it: this hook feeds
      // ChatTab, and a per-keystroke subscription here re-renders the whole
      // message list under it.
      const text =
        typeof textOrUpdater === "function"
          ? textOrUpdater(useComposerStore.getState().composerText)
          : textOrUpdater;
      setComposerGroupMentionTokens((tokens) => pruneComposerGroupMentionTokens({ text, tokens }));
      const liveAgentMentionTokens = pruneComposerAgentMentionTokens({
        text,
        tokens: composerAgentMentionTokens,
      });
      if (liveAgentMentionTokens.length !== composerAgentMentionTokens.length) {
        setComposerAgentMentionTokens(liveAgentMentionTokens);
      }
      setComposerText(text);
    },
    [composerAgentMentionTokens, setComposerText],
  );

  const removeComposerFile = useCallback(
    (idx: number) => {
      setComposerFiles(composerFiles.filter((_, i) => i !== idx));
    },
    [composerFiles, setComposerFiles],
  );

  const sendMessage = useCallback(async () => {
    if (sendInFlightRef.current) return; // keyboard shortcut can bypass UI state; keep send single-flight locally
    if (!selectedGroupId) return;
    const latestSelectedGroupId = String(useGroupStore.getState().selectedGroupId || "").trim();
    if (latestSelectedGroupId !== selectedGroupId) return;
    const composerStateSnapshot = useComposerStore.getState();
    const routingSnapshot = buildComposerSendRoutingSnapshot({
      selectedGroupId: latestSelectedGroupId,
      activeGroupId: composerStateSnapshot.activeGroupId,
      destGroupId: composerStateSnapshot.destGroupId,
    });
    if (!routingSnapshot.composerGroupSettled) return;
    const originGroupId = routingSnapshot.selectedGroupId;
    const draftTextSnapshot = String(composerStateSnapshot.composerText || "").trim();
    const draftFilesSnapshot = composerStateSnapshot.composerFiles.slice();
    if (!draftTextSnapshot && draftFilesSnapshot.length === 0) return;
    const dstGroup = routingSnapshot.destGroupId;
    const isCrossGroup = routingSnapshot.isCrossGroup;
    const selectedRemoteGroupIdsSnapshot = selectedRemoteGroupIds.slice();
    const toTextSnapshot = composerStateSnapshot.toText;
    const localToTokensSnapshot = buildComposerSendRecipientTokens({
      toText: toTextSnapshot,
      isCrossGroup: false,
      validRecipientSet,
      crossGroupValidRecipientSet,
    });
    const crossToTokensSnapshot = buildComposerSendRecipientTokens({
      toText: toTextSnapshot,
      isCrossGroup: true,
      validRecipientSet,
      crossGroupValidRecipientSet,
    });
    const sendPlanTargets = buildComposerSendPlanTargets({
      selectedGroupId: originGroupId,
      dstGroupId: dstGroup,
      isCrossGroup,
      text: composerStateSnapshot.composerText,
      groupMentionTokens: composerGroupMentionTokens,
      groups: composerRouteGroups,
      remoteGroupIds: selectedRemoteGroupIdsSnapshot,
      includeSelectedGroup:
        selectedRemoteGroupIdsSnapshot.length > 0 && localToTokensSnapshot.length > 0,
    });
    const sendsCrossGroup = sendPlanTargets.some((target) => target.isCrossGroup);
    const sendsLocal = sendPlanTargets.some((target) => !target.isCrossGroup);
    const { text: txt, files: composerFilesSnapshot } = prepareComposerMessage({
      text: draftTextSnapshot,
      files: draftFilesSnapshot,
      targets: sendPlanTargets,
    });
    const slashGuardSendGroupId = sendsCrossGroup
      ? sendPlanTargets.find((target) => target.isCrossGroup)?.groupId || dstGroup
      : dstGroup;
    if (
      await tryExecuteSlashCommand({
        text: composerStateSnapshot.composerText,
        composerFilesCount: composerFilesSnapshot.length,
        hasReplyTarget: !!composerStateSnapshot.replyTarget,
        replyTarget: composerStateSnapshot.replyTarget,
        hasQuotedPresentationRef: !!composerStateSnapshot.quotedPresentationRef,
        hasQuotedVoiceDocumentRef: !!composerStateSnapshot.quotedVoiceDocumentRef,
        sendGroupId: slashGuardSendGroupId,
      })
    ) {
      return;
    }
    const replyTargetSnapshot = composerStateSnapshot.replyTarget;
    const remoteReplyDstGroupId = String(replyTargetSnapshot?.remoteDstGroupId || "").trim();
    const remoteReplyDstTo = Array.isArray(replyTargetSnapshot?.remoteDstTo)
      ? replyTargetSnapshot.remoteDstTo.map((token) => String(token || "").trim()).filter(Boolean)
      : [];
    const quotedPresentationRefSnapshot = composerStateSnapshot.quotedPresentationRef;
    const quotedVoiceDocumentRefSnapshot = composerStateSnapshot.quotedVoiceDocumentRef;
    const refsSnapshot: MessageRef[] = [
      ...(quotedPresentationRefSnapshot ? [quotedPresentationRefSnapshot] : []),
      ...(quotedVoiceDocumentRefSnapshot ? [quotedVoiceDocumentRefSnapshot] : []),
      ...buildComposerLocalGroupRouteRefs({
        text: composerStateSnapshot.composerText,
        selectedGroupId,
        tokens: composerGroupMentionTokens,
        groups: composerRouteGroups,
      }),
      ...buildComposerGroupBridgeRouteRefs({
        text: composerStateSnapshot.composerText,
        tokens: composerGroupMentionTokens,
        groups: composerRouteGroups,
      }),
    ];
    const messageModeSnapshot = replyTargetSnapshot
      ? normalizeReplyMessageMode(composerStateSnapshot.messageMode)
      : composerStateSnapshot.messageMode;
    const groupMentionTokensSnapshot = composerGroupMentionTokens;
    const agentMentionTokensSnapshot = composerAgentMentionTokens;
    const assistantTargets =
      sendsLocal && !sendsCrossGroup && messageModeSnapshot !== "mail"
        ? resolveAssistantTargets(localToTokensSnapshot)
        : [];

    const localId = `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const insertLocalAssistantPlaceholders = () => {
      for (const placeholder of buildAssistantPlaceholders(
        assistantTargets,
        localId,
        selectedGroupId,
      )) {
        upsertStreamingEvent(placeholder, selectedGroupId);
      }
    };

    const clearLocalAssistantPlaceholders = () => {
      removeStreamingEventsByPrefix(`local:${localId}:`, selectedGroupId);
    };

    const restoreComposerState = () => {
      restoreFailedSendComposerState(
        {
          originGroupId,
          composerText: draftTextSnapshot,
          composerFiles: draftFilesSnapshot,
          toText: toTextSnapshot,
          replyTarget: replyTargetSnapshot,
          quotedPresentationRef: quotedPresentationRefSnapshot,
          quotedVoiceDocumentRef: quotedVoiceDocumentRefSnapshot,
          messageMode: messageModeSnapshot,
        },
        {
          setComposerText,
          setComposerFiles,
          setReplyTarget,
          setQuotedPresentationRef,
          setQuotedVoiceDocumentRef,
          setMessageMode,
          setToText,
          upsertDraft,
        },
      );
      setComposerGroupMentionTokens(groupMentionTokensSnapshot);
      setComposerAgentMentionTokens(agentMentionTokensSnapshot);
      setSelectedRemoteGroupIds(selectedRemoteGroupIdsSnapshot);
    };

    const applyImmediateComposerFeedback = (shouldLockBottom: boolean) => {
      clearComposer();
      setComposerGroupMentionTokens([]);
      setComposerAgentMentionTokens([]);
      setSelectedRemoteGroupIds([]);
      if (chatAtBottomRef) chatAtBottomRef.current = shouldLockBottom;
      if (selectedGroupId) {
        setShowScrollButton(selectedGroupId, !shouldLockBottom);
      }
      if (shouldLockBottom) {
        nextSendScrollRequestIdRef.current += 1;
        setSendScrollRequest(
          createChatSendScrollRequest(
            nextSendScrollRequestIdRef.current,
            selectedGroupId,
            chatViewKey,
          ),
        );
      }
    };

    if (replyTargetSnapshot && sendsCrossGroup && !remoteReplyDstGroupId) {
      showError("Cross-group send does not support replies.");
      setDestGroupId(selectedGroupId);
      return;
    }
    if (quotedPresentationRefSnapshot && sendsCrossGroup) {
      showError("Cross-group send does not support quoted presentation views.");
      setDestGroupId(selectedGroupId);
      return;
    }
    if (quotedVoiceDocumentRefSnapshot && sendsCrossGroup) {
      showError("Cross-group send does not support quoted voice documents.");
      setDestGroupId(selectedGroupId);
      return;
    }
    if (
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: composerFilesSnapshot.length,
        targets: sendPlanTargets,
      })
    ) {
      showError(
        "Local cross-group send does not support attachments yet. Use a remote Group Bridge target or send without attachments.",
      );
      return;
    }

    // Preserve reading intent before optimistic rows change the list geometry.
    const shouldLockBottomAfterSend = shouldFollowCurrentSend();

    // Failed optimistic sends restore the original composer snapshot.
    if (sendsLocal && !sendsCrossGroup) {
      const optimisticEvent = buildOptimisticMessage({
        localId,
        groupId: selectedGroupId,
        text: txt,
        to: localToTokensSnapshot,
        messageMode: messageModeSnapshot,
        replyTarget: replyTargetSnapshot,
        refs: refsSnapshot,
        files: composerFilesSnapshot,
      });
      enqueueOutbox(selectedGroupId, localId, optimisticEvent);
      insertLocalAssistantPlaceholders();
    }

    applyImmediateComposerFeedback(shouldLockBottomAfterSend);
    sendInFlightRef.current = true;
    let successfulSendCount = 0;
    try {
      const dispatched = await dispatchPreparedMessage({
        selectedGroupId,
        text: txt,
        localTo: localToTokensSnapshot,
        crossTo: crossToTokensSnapshot,
        files: composerFilesSnapshot,
        messageMode: messageModeSnapshot,
        localId,
        refs: refsSnapshot,
        replyTarget: replyTargetSnapshot,
        remoteReplyGroupId: remoteReplyDstGroupId,
        remoteReplyTo: remoteReplyDstTo,
        sendPlanTargets,
        sendsCrossGroup,
      });
      const resp = dispatched.response;
      successfulSendCount = dispatched.successfulSendCount;
      if (!resp.ok) {
        // Pending-only outbox: failed sends roll back to the composer.
        removeOutbox(selectedGroupId, localId);
        clearLocalAssistantPlaceholders();
        const shouldRestoreComposer = shouldRestoreComposerAfterFailedSend(successfulSendCount);
        if (shouldRestoreComposer) restoreComposerState();
        const sendError = resp.error || { code: "send_failed", message: "send failed" };
        showError(formatSendMessageError({ code: sendError.code, message: sendError.message, t }));
        return;
      }
      const canonicalEvent =
        sendsLocal &&
        !sendsCrossGroup &&
        resp.result &&
        typeof resp.result === "object" &&
        "event" in resp.result
          ? (resp.result.event as LedgerEvent | null | undefined)
          : undefined;

      if (sendsCrossGroup) {
        removeOutbox(selectedGroupId, localId);
      }
      if (canonicalEvent && !sendsCrossGroup) {
        const reconciliation = reconcileCanonicalOutboxEvent(canonicalEvent, selectedGroupId);
        const canonicalEventId = String(reconciliation.event.id || "").trim();
        appendEvent(reconciliation.event, selectedGroupId);
        if (canonicalEventId) {
          promoteStreamingEventsByPrefix(`local:${localId}:`, canonicalEventId, selectedGroupId);
        }
        completeCanonicalOutboxReconciliation(selectedGroupId, reconciliation);
      }
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
      if (selectedGroupId) {
        setChatUnreadCount(selectedGroupId, 0);
        setChatFilter(selectedGroupId, "all");
        setChatMobileSurface(selectedGroupId, "messages");
      }
      onMessageSent?.();
    } catch (error) {
      const message = error instanceof Error ? error.message : "send failed";
      // Pending-only outbox: failed sends roll back to the composer.
      removeOutbox(selectedGroupId, localId);
      clearLocalAssistantPlaceholders();
      const shouldRestoreComposer = shouldRestoreComposerAfterFailedSend(successfulSendCount);
      if (shouldRestoreComposer) restoreComposerState();
      showError(message);
    } finally {
      sendInFlightRef.current = false;
    }
  }, [
    selectedGroupId,
    tryExecuteSlashCommand,
    validRecipientSet,
    crossGroupValidRecipientSet,
    inChatWindow,
    appendEvent,
    enqueueOutbox,
    removeOutbox,
    showError,
    clearComposer,
    setComposerText,
    setComposerFiles,
    setReplyTarget,
    setQuotedPresentationRef,
    setQuotedVoiceDocumentRef,
    setMessageMode,
    setToText,
    setDestGroupId,
    upsertDraft,
    clearDraft,
    closeChatWindow,
    fileInputRef,
    shouldFollowCurrentSend,
    setChatFilter,
    setChatMobileSurface,
    setShowScrollButton,
    setChatUnreadCount,
    onMessageSent,
    promoteStreamingEventsByPrefix,
    removeStreamingEventsByPrefix,
    resolveAssistantTargets,
    upsertStreamingEvent,
    t,
    composerGroupMentionTokens,
    composerAgentMentionTokens,
    composerRouteGroups,
    selectedRemoteGroupIds,
    chatAtBottomRef,
    chatViewKey,
  ]);

  const {
    copyMessageLink,
    copyMessageText,
    startReply,
    cancelReply,
    showRecipients,
    relayMessage,
    openSourceMessage,
    exitChatWindow,
    handleScrollButtonClick,
    handleScrollChange,
    handleScrollSnapshot,
    addAgent,
    loadCurrentGroupHistory,
  } = useChatMessageActions({
    selectedGroupId,
    actors,
    groups,
    groupSettings,
    composerRef,
    setChatAtBottom,
    hasForeman,
    inChatWindow,
    t,
    showError,
    showNotice,
    setDestGroupId,
    setReplyToText,
    setReplyTarget,
    setRecipientsModal,
    setRelayModal,
    openChatWindow,
    closeChatWindow,
    setShowScrollButton,
    setChatUnreadCount,
    setChatScrollSnapshot,
    setNewActorRole,
    openModal,
    loadMoreHistory,
  });

  // ============ Return ============

  return {
    // Chat state
    chatMessages,
    suggestionSourceMessages: unfilteredLiveChatMessages,
    liveWorkEvents,
    hasAnyChatMessages,
    chatFilter,
    setChatFilter: updateChatFilter,
    chatViewKey,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatInitialScrollAnchorId,
    chatInitialScrollAnchorOffsetPx,
    chatInitialScrollOffsetPx,
    chatHighlightEventId,
    inChatWindow,
    isLoadingHistory: effectiveIsLoadingHistory,
    hasMoreHistory: effectiveHasMoreHistory,
    loadMoreHistory: inChatWindow ? undefined : loadCurrentGroupHistory,
    chatEmptyState,

    // UI state
    busy,
    showScrollButton,
    chatUnreadCount,
    sendScrollRequest,
    consumeSendScrollRequest,

    // Setup checklist
    showSetupCard,
    needsScope,
    needsActors,
    needsStart,
    hasForeman,

    // Composer state
    setComposerText: syncMentionRecipientsFromComposerText,
    composerGroupMentionTokens,
    setComposerGroupMentionTokens,
    composerAgentMentionTokens,
    setComposerAgentMentionTokens,
    composerFiles,
    setComposerFiles,
    removeComposerFile,
    replyTarget,
    quotedPresentationRef,
    quotedVoiceDocumentRef,
    cancelReply,
    clearQuotedPresentationRef: () => setQuotedPresentationRef(null),
    setQuotedVoiceDocumentRef,
    clearQuotedVoiceDocumentRef: () => setQuotedVoiceDocumentRef(null),
    toTokens,
    toggleRecipient,
    selectedRemoteGroupIds,
    toggleRemoteGroupRecipient,
    clearRecipients,
    messageMode,
    setMessageMode,
    destGroupId: sendGroupId,
    setDestGroupId,
    composerGroupSettled,
    composerRouteGroups,
    mentionSuggestions,

    // Agent state
    agentStates,
    taskById,

    // Actions
    sendMessage,
    slashCommands,
    copyMessageLink,
    copyMessageText,
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
