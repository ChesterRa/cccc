import { requestWorkspaceNavigation } from "../stores/workspaceNavigation";
// AppModals renders all modal components in one place.
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { SearchModal } from "./SearchModal";
import { MobileMenuSheet } from "./layout/MobileMenuSheet";
import { CreateGroupModal } from "./modals/CreateGroupModal";
import { useCreateGroupDirectoryBrowser } from "./modals/useCreateGroupDirectoryBrowser";
import { ActorConfigModal } from "./modals/ActorConfigModal";
import { useAppModalActorActions } from "./modals/useAppModalActorActions";
import { GroupEditModal } from "./modals/GroupEditModal";
import { InboxModal } from "./modals/InboxModal";
import { PresentationPinModal } from "./presentation/PresentationPinModal";
import { RelayMessageModal } from "./modals/RelayMessageModal";
import { RecipientsModal } from "./modals/RecipientsModal";
import {
  openContextModalData,
  syncContextModalData,
  type ContextModalFetch,
} from "../features/contextModal/contextRead";
import { findPresentationSlot } from "../utils/presentation";
import { buildPresentationRefForSlot } from "../utils/presentationRefs";
import { formatGroupSettingsUpdateError } from "../utils/groupSettingsErrors";
import { appendQuotedOriginalPerspective, getMessageInsight } from "../utils/messagePerspective";
import { projectCrossGroupRecipients, projectMessageMode } from "../utils/crossGroupRecipients";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useComposerStore,
  useInboxStore,
  useFormStore,
  selectChatBucketState,
} from "../stores";
import { getRecipientActorIdsForEvent } from "../hooks/useSSE";
import { getChatSession } from "../stores/useUIStore";
import * as api from "../services/api";
import {
  LedgerEvent,
  GroupSettings,
  ChatMessageData,
  PresentationMessageRef,
  TextScale,
  Theme,
} from "../types";
import { useShallow } from "zustand/react/shallow";

const ContextModal = lazy(() =>
  import("./ContextModal/index").then((module) => ({ default: module.ContextModal })),
);
const SettingsModal = lazy(() =>
  import("./SettingsModal").then((module) => ({ default: module.SettingsModal })),
);
const PresentationViewerModal = lazy(() =>
  import("./presentation/PresentationViewerModal").then((module) => ({
    default: module.PresentationViewerModal,
  })),
);

interface AppModalsProps {
  codexVoice: import("../features/codexVoice/useCodexVoiceShell").CodexVoiceShellState;
  isDark: boolean;
  theme: Theme;
  textScale: TextScale;
  readOnly?: boolean;
  ccccHome: string;
  composerRef: React.RefObject<HTMLTextAreaElement | null>;
  onStartReply: (ev: LedgerEvent) => void;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  onDeleteGroup: (groupId: string) => Promise<void>;
  fetchContext: ContextModalFetch;
  canManageGroups: boolean;
  accountLabel?: string | null;
  onNativeActorCreated?: (
    visit: import("../hooks/useAppTabState").CreatedNativeActorNavigation,
  ) => void;
}

function sortPresentationSlotIds(slotIds: string[]): string[] {
  return [...slotIds].sort((left, right) => {
    const leftIndex = Number(String(left || "").replace("slot-", "")) || 0;
    const rightIndex = Number(String(right || "").replace("slot-", "")) || 0;
    return leftIndex - rightIndex;
  });
}

function LazyModalFallback({ isDark: _ }: { isDark?: boolean }) {
  return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/40 backdrop-blur-[2px]">
      <div className="rounded-2xl px-5 py-4 text-sm shadow-xl glass-modal min-w-[120px] flex items-center justify-center font-medium text-[var(--color-text-secondary)]">
        <span className="flex items-center gap-2">
          <span className="h-4 w-4 animate-spin rounded-full border-2 border-[var(--color-text-muted)] border-t-transparent" />
          <span>Loading...</span>
        </span>
      </div>
    </div>
  );
}

export function AppModals({
  codexVoice,
  isDark,
  theme,
  textScale,
  readOnly,
  ccccHome,
  composerRef,
  onStartReply,
  onThemeChange,
  onTextScaleChange,
  onDeleteGroup,
  onNativeActorCreated,
  fetchContext,
  canManageGroups,
  accountLabel,
}: AppModalsProps) {
  const { t } = useTranslation(["actors", "chat", "modals"]);
  // Stores
  const {
    groups,
    selectedGroupId,
    groupDoc,
    events,
    chatWindow,
    senderHistoryEvents,
    actors,
    groupContext,
    groupSettings,
    groupPresentation,
    runtimes,
    runtimeDetectionStatus,
    setSelectedGroupId,
    setGroupContext,
    setGroupSettings,
    setGroupPresentation,
    refreshGroups,
    refreshSettings,
    refreshActors,
    loadGroup,
    openChatWindow,
    mergeEventStatuses,
  } = useGroupStore(
    useShallow((s) => {
      const bucket = selectChatBucketState(s, s.selectedGroupId);
      return {
        groups: s.groups,
        selectedGroupId: s.selectedGroupId,
        groupDoc: s.groupDoc,
        events: bucket.events,
        chatWindow: bucket.chatWindow,
        senderHistoryEvents: bucket.senderHistory?.events,
        actors: s.actors,
        groupContext: s.groupContext,
        groupSettings: s.groupSettings,
        groupPresentation: s.groupPresentation,
        runtimes: s.runtimes,
        runtimeDetectionStatus: s.runtimeDetectionStatus,
        setSelectedGroupId: s.setSelectedGroupId,
        setGroupContext: s.setGroupContext,
        setGroupSettings: s.setGroupSettings,
        setGroupPresentation: s.setGroupPresentation,
        refreshGroups: s.refreshGroups,
        refreshSettings: s.refreshSettings,
        refreshActors: s.refreshActors,
        loadGroup: s.loadGroup,
        openChatWindow: s.openChatWindow,
        mergeEventStatuses: s.mergeEventStatuses,
      };
    }),
  );

  const {
    busy,
    isSmallScreen,
    chatSessions,
    setBusy,
    showError,
    showNotice,
    setActiveTab,
    setChatMobileSurface,
    setChatPresentationDockOpen,
    setChatPresentationDisplayMode,
  } = useUIStore(
    useShallow((s) => ({
      busy: s.busy,
      isSmallScreen: s.isSmallScreen,
      chatSessions: s.chatSessions,
      setBusy: s.setBusy,
      showError: s.showError,
      showNotice: s.showNotice,
      setActiveTab: s.setActiveTab,
      setChatMobileSurface: s.setChatMobileSurface,
      setChatPresentationDockOpen: s.setChatPresentationDockOpen,
      setChatPresentationDisplayMode: s.setChatPresentationDisplayMode,
    })),
  );

  const {
    modals,
    recipientsEventId: _recipientsEventId,
    relayEventId,
    relaySource,
    presentationViewer,
    presentationPin,
    editingActor,
    openModal,
    closeModal,
    setRecipientsModal,
    setRelayModal,
    setPresentationViewer,
    setPresentationPin,
    clearPresentationSlotAttention,
    setEditingActor,
    clearContextTask,
  } = useModalStore(
    useShallow((s) => ({
      modals: s.modals,
      recipientsEventId: s.recipientsEventId,
      relayEventId: s.relayEventId,
      relaySource: s.relaySource,
      presentationViewer: s.presentationViewer,
      presentationPin: s.presentationPin,
      editingActor: s.editingActor,
      openModal: s.openModal,
      closeModal: s.closeModal,
      setRecipientsModal: s.setRecipientsModal,
      setRelayModal: s.setRelayModal,
      setPresentationViewer: s.setPresentationViewer,
      setPresentationPin: s.setPresentationPin,
      clearPresentationSlotAttention: s.clearPresentationSlotAttention,
      setEditingActor: s.setEditingActor,
      clearContextTask: s.clearContextTask,
    })),
  );
  const editingActorSection = useModalStore((state) => state.editingActorSection);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const contextTaskId = useModalStore((state) => state.contextTaskId);

  const { inboxTarget, inboxMessages, setInboxMessages, clearInbox } = useInboxStore(
    useShallow((s) => ({
      inboxTarget: s.inboxTarget,
      inboxMessages: s.inboxMessages,
      setInboxMessages: s.setInboxMessages,
      clearInbox: s.clearInbox,
    })),
  );
  useEffect(() => {
    if (inboxTarget && inboxTarget.groupId !== selectedGroupId) {
      clearInbox();
      closeModal("inbox");
    }
  }, [inboxTarget, selectedGroupId, clearInbox, closeModal]);
  const setQuotedPresentationRef = useComposerStore((state) => state.setQuotedPresentationRef);
  const setComposerDestGroupId = useComposerStore((state) => state.setDestGroupId);
  const [messageActionBusy, setMessageActionBusy] = useState("");

  const preferredPresentationSurface = selectedGroupId
    ? !isSmallScreen &&
      getChatSession(selectedGroupId, chatSessions).presentationDisplayMode === "split"
      ? "split"
      : "modal"
    : "modal";

  const {
    editGroupTitle,
    editGroupTopic,
    setEditGroupTitle,
    setEditGroupTopic,
    editActorRuntime,
    editActorCommand,
    editActorTitle,
    editActorNotes,
    editActorCapabilityAutoloadText,
    setEditActorRuntime,
    setEditActorCommand,
    setEditActorTitle,
    setEditActorNotes,
    setEditActorCapabilityAutoloadText,
    newActorId,
    newActorRole,
    newActorRuntime,
    newActorCommand,
    newActorUseDefaultCommand,
    newActorSecretsSetText,
    newActorCapabilityAutoloadText,
    newActorNotes,
    newActorUseProfile,
    newActorProfileId,
    addActorError,
    setNewActorId,
    setNewActorRole,
    setNewActorRuntime,
    setNewActorCommand,
    setNewActorUseDefaultCommand,
    setNewActorSecretsSetText,
    setNewActorCapabilityAutoloadText,
    setNewActorNotes,
    setNewActorUseProfile,
    setNewActorProfileId,
    setAddActorError,
    resetAddActorForm,
    createGroupPath,
    createGroupName,
    dirItems,
    dirSuggestions,
    currentDir,
    parentDir,
    showDirBrowser,
    setCreateGroupPath,
    setCreateGroupName,
    resetCreateGroupForm,
  } = useFormStore(
    useShallow((s) => ({
      editGroupTitle: s.editGroupTitle,
      editGroupTopic: s.editGroupTopic,
      setEditGroupTitle: s.setEditGroupTitle,
      setEditGroupTopic: s.setEditGroupTopic,
      editActorRuntime: s.editActorRuntime,
      editActorCommand: s.editActorCommand,
      editActorTitle: s.editActorTitle,
      editActorNotes: s.editActorNotes,
      editActorCapabilityAutoloadText: s.editActorCapabilityAutoloadText,
      setEditActorRuntime: s.setEditActorRuntime,
      setEditActorCommand: s.setEditActorCommand,
      setEditActorTitle: s.setEditActorTitle,
      setEditActorNotes: s.setEditActorNotes,
      setEditActorCapabilityAutoloadText: s.setEditActorCapabilityAutoloadText,
      newActorId: s.newActorId,
      newActorRole: s.newActorRole,
      newActorRuntime: s.newActorRuntime,
      newActorCommand: s.newActorCommand,
      newActorUseDefaultCommand: s.newActorUseDefaultCommand,
      newActorSecretsSetText: s.newActorSecretsSetText,
      newActorCapabilityAutoloadText: s.newActorCapabilityAutoloadText,
      newActorNotes: s.newActorNotes,
      newActorUseProfile: s.newActorUseProfile,
      newActorProfileId: s.newActorProfileId,
      addActorError: s.addActorError,
      setNewActorId: s.setNewActorId,
      setNewActorRole: s.setNewActorRole,
      setNewActorRuntime: s.setNewActorRuntime,
      setNewActorCommand: s.setNewActorCommand,
      setNewActorUseDefaultCommand: s.setNewActorUseDefaultCommand,
      setNewActorSecretsSetText: s.setNewActorSecretsSetText,
      setNewActorCapabilityAutoloadText: s.setNewActorCapabilityAutoloadText,
      setNewActorNotes: s.setNewActorNotes,
      setNewActorUseProfile: s.setNewActorUseProfile,
      setNewActorProfileId: s.setNewActorProfileId,
      setAddActorError: s.setAddActorError,
      resetAddActorForm: s.resetAddActorForm,
      createGroupPath: s.createGroupPath,
      createGroupName: s.createGroupName,
      dirItems: s.dirItems,
      dirSuggestions: s.dirSuggestions,
      currentDir: s.currentDir,
      parentDir: s.parentDir,
      showDirBrowser: s.showDirBrowser,
      setCreateGroupPath: s.setCreateGroupPath,
      setCreateGroupName: s.setCreateGroupName,
      resetCreateGroupForm: s.resetCreateGroupForm,
    })),
  );

  const directoryBrowser = useCreateGroupDirectoryBrowser();
  const handleActorScopeAttached = useCallback(
    (groupId: string, scope: api.AttachScopeResult["scope"]) => {
      useGroupStore.setState((state) => {
        const doc = state.groupDoc;
        if (state.selectedGroupId !== groupId || doc?.group_id !== groupId) return state;
        const scopes = doc.scopes || [];
        return {
          groupDoc: {
            ...doc,
            scopes: scopes.some((item) => item.scope_key === scope.scope_key)
              ? scopes.map((item) => (item.scope_key === scope.scope_key ? scope : item))
              : [...scopes, scope],
          },
        };
      });
    },
    [],
  );
  const [presentationViewerCacheByGroup, setPresentationViewerCacheByGroup] = useState<
    Record<string, string[]>
  >({});
  const {
    actorProfiles,
    actorProfilesBusy,
    editActorNotesBusy,
    savedActorNotes,
    loadActorProfiles,
    handleSaveEditActorOnly,
    handleSaveEditActorAndRestart,
    handleSaveEditActorAsProfile,
    handleAddActor,
    handleSaveNewActorAsProfile,
    suggestedActorId,
    canAddActor,
    addActorDisabledReason,
    handleCloseAddActor,
    handleCancelEditActor,
  } = useAppModalActorActions({
    selectedGroupId,
    actors,
    runtimes,
    runtimeDetectionStatus,
    busy,
    setBusy,
    showError,
    showNotice,
    addActorModalOpen: modals.addActor,
    editingActor,
    setEditingActor,
    closeModal,
    refreshActors,
    onNativeActorCreated,
    editActorRuntime,
    editActorCommand,
    editActorTitle,
    editActorNotes,
    editActorCapabilityAutoloadText,
    setEditActorNotes,
    newActorId,
    newActorRole,
    newActorRuntime,
    newActorCommand,
    newActorUseDefaultCommand,
    newActorSecretsSetText,
    newActorCapabilityAutoloadText,
    newActorNotes,
    newActorUseProfile,
    newActorProfileId,
    setAddActorError,
    resetAddActorForm,
    t,
  });

  const rememberPresentationViewerSlot = useCallback((groupId: string, slotId: string) => {
    const normalizedGroupId = String(groupId || "").trim();
    const normalizedSlotId = String(slotId || "").trim();
    if (!normalizedGroupId || !normalizedSlotId) return;
    setPresentationViewerCacheByGroup((current) => {
      const existing = current[normalizedGroupId] || [];
      if (existing.includes(normalizedSlotId)) return current;
      return {
        ...current,
        [normalizedGroupId]: sortPresentationSlotIds([...existing, normalizedSlotId]),
      };
    });
  }, []);

  const forgetPresentationViewerSlot = useCallback((groupId: string, slotId: string) => {
    const normalizedGroupId = String(groupId || "").trim();
    const normalizedSlotId = String(slotId || "").trim();
    if (!normalizedGroupId || !normalizedSlotId) return;
    setPresentationViewerCacheByGroup((current) => {
      const existing = current[normalizedGroupId] || [];
      if (!existing.includes(normalizedSlotId)) return current;
      const nextSlots = existing.filter((item) => item !== normalizedSlotId);
      if (nextSlots.length === existing.length) return current;
      if (nextSlots.length === 0) {
        const next = { ...current };
        delete next[normalizedGroupId];
        return next;
      }
      return { ...current, [normalizedGroupId]: nextSlots };
    });
  }, []);

  useEffect(() => {
    if (presentationViewer && presentationViewer.groupId !== selectedGroupId) {
      setPresentationViewer(null);
    }
  }, [presentationViewer, selectedGroupId, setPresentationViewer]);

  useEffect(() => {
    if (!presentationViewer) return;
    if (presentationViewer.groupId !== selectedGroupId) return;
    if (findPresentationSlot(groupPresentation, presentationViewer.slotId)?.card) return;
    if (presentationViewer.focusRef) return;
    setPresentationViewer(null);
  }, [groupPresentation, presentationViewer, selectedGroupId, setPresentationViewer]);

  useEffect(() => {
    if (!presentationViewer) return;
    rememberPresentationViewerSlot(presentationViewer.groupId, presentationViewer.slotId);
  }, [presentationViewer, rememberPresentationViewerSlot]);

  useEffect(() => {
    if (presentationPin && presentationPin.groupId !== selectedGroupId) {
      setPresentationPin(null);
    }
  }, [presentationPin, selectedGroupId, setPresentationPin]);

  const presentationViewerSlotIds = useMemo(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return [];
    const cachedSlots = (presentationViewerCacheByGroup[gid] || []).filter(
      (slotId) => !!findPresentationSlot(groupPresentation, slotId)?.card,
    );
    const activeSlotId =
      presentationViewer && presentationViewer.groupId === gid
        ? String(presentationViewer.slotId || "").trim()
        : "";
    if (activeSlotId && !cachedSlots.includes(activeSlotId)) {
      cachedSlots.push(activeSlotId);
    }
    return sortPresentationSlotIds(cachedSlots);
  }, [groupPresentation, presentationViewer, presentationViewerCacheByGroup, selectedGroupId]);

  // Computed
  const selectedGroupRunning = useGroupStore(
    (s) => s.groups.find((g) => String(g.group_id || "") === s.selectedGroupId)?.running ?? false,
  );
  const hasForeman = actors.some((a) => a.role === "foreman");

  // Compute messageMeta for RecipientsModal (moved from App.tsx)
  const messageMetaEvent = useMemo(() => {
    if (!_recipientsEventId) return null;
    const liveHit = events.find(
      (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId,
    );
    if (liveHit) return liveHit;
    const windowEvents = Array.isArray(chatWindow?.events) ? chatWindow.events : [];
    const windowHit = windowEvents.find(
      (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId,
    );
    if (windowHit) return windowHit;
    const filteredEvents = Array.isArray(senderHistoryEvents) ? senderHistoryEvents : [];
    return (
      filteredEvents.find(
        (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId,
      ) || null
    );
  }, [chatWindow?.events, events, senderHistoryEvents, _recipientsEventId]);

  const messageMeta = useMemo(() => {
    if (!messageMetaEvent) return null;
    // Type guard: ensure data.to is an array.
    const metaData = messageMetaEvent.data as ChatMessageData | undefined;
    const toTokensList = String(metaData?.dst_group_id || "").trim()
      ? projectCrossGroupRecipients(metaData)
      : (Array.isArray(metaData?.to) ? metaData.to : [])
          .map((x) => String(x || "").trim())
          .filter((s) => s.length > 0);
    const toLabel = toTokensList.length > 0 ? toTokensList.join(", ") : "@all";
    const messageMode = projectMessageMode(metaData) || "send";
    const rs =
      messageMetaEvent._read_status && typeof messageMetaEvent._read_status === "object"
        ? messageMetaEvent._read_status
        : null;

    const os =
      messageMetaEvent._obligation_status && typeof messageMetaEvent._obligation_status === "object"
        ? messageMetaEvent._obligation_status
        : null;
    if (metaData?.dst_instance_id) {
      const delivery = messageMetaEvent._connect_delivery || { state: "queued" as const };
      const titles = metaData.dst_actor_titles || {};
      return {
        sourceEventId: String(messageMetaEvent.id || ""),
        toLabel: toTokensList.map((id) => titles[id] || id).join(", "),
        entries: toTokensList.map((id) => ({
          id,
          label: titles[id] || id,
          cleared:
            messageMode === "request_reply"
              ? !!os?.[id]?.replied || !!os?.[id]?.cancelled
              : delivery.state === "sent",
          deliveryState: "",
          read: false,
          replied: !!os?.[id]?.replied,
          replyRequested: messageMode === "request_reply",
          cancelled: !!os?.[id]?.cancelled,
        })),
        statusKind: messageMode === "request_reply" ? ("reply" as const) : ("delivery" as const),
        messageMode,
        remoteDelivery: delivery,
        canCancelReply:
          messageMode === "request_reply" &&
          !messageMetaEvent._connect_cancellation &&
          toTokensList.some((id) => !os?.[id]?.replied && !os?.[id]?.cancelled),
      };
    }
    if (os) {
      const recipientIds = Object.keys(os);
      const recipientIdSet = new Set(recipientIds);
      const entries = [
        ...actors
          .map((a) => String(a.id || ""))
          .filter((id) => id && recipientIdSet.has(id))
          .map((id) => {
            const deliveryState = String(os[id]?.delivery_state || "");
            return {
              id,
              cleared:
                messageMode === "mail"
                  ? !!rs?.[id]
                  : messageMode === "request_reply"
                    ? !!os[id]?.replied || !!os[id]?.cancelled
                    : ["accepted", "ambiguous"].includes(deliveryState),
              deliveryState,
              read: !!rs?.[id],
              replied: !!os[id]?.replied,
              replyRequested: !!os[id]?.reply_requested,
              cancelled: !!os[id]?.cancelled,
            };
          }),
        recipientIdSet.has("user")
          ? {
              id: "user",
              cleared:
                messageMode === "mail"
                  ? !!rs?.user
                  : messageMode === "request_reply"
                    ? !!os.user?.replied || !!os.user?.cancelled
                    : ["accepted", "ambiguous"].includes(String(os.user?.delivery_state || "")),
              deliveryState: String(os["user"]?.delivery_state || ""),
              read: !!rs?.user,
              replied: !!os["user"]?.replied,
              replyRequested: !!os["user"]?.reply_requested,
              cancelled: !!os["user"]?.cancelled,
            }
          : null,
      ].filter(Boolean) as Array<{
        id: string;
        cleared: boolean;
        deliveryState: string;
        read: boolean;
        replied: boolean;
        replyRequested: boolean;
        cancelled: boolean;
      }>;
      return {
        sourceEventId: String(messageMetaEvent.id || ""),
        toLabel,
        entries,
        statusKind:
          messageMode === "mail"
            ? ("read" as const)
            : messageMode === "request_reply"
              ? ("reply" as const)
              : ("delivery" as const),
        messageMode,
        canCancelReply: recipientIds.some(
          (id) => !!os[id]?.reply_requested && !os[id]?.replied && !os[id]?.cancelled,
        ),
      };
    }

    const recipientIds = rs
      ? Object.keys(rs)
      : getRecipientActorIdsForEvent(messageMetaEvent, actors);
    const recipientIdSet = new Set(recipientIds);
    const entries = actors
      .map((a) => String(a.id || ""))
      .filter((id) => id && recipientIdSet.has(id))
      .map((id) => ({
        id,
        cleared: !!(rs && rs[id]),
        deliveryState: "",
        read: !!(rs && rs[id]),
        replied: false,
        replyRequested: false,
        cancelled: false,
      }));

    return {
      sourceEventId: String(messageMetaEvent.id || ""),
      toLabel,
      entries,
      statusKind:
        messageMode === "mail"
          ? ("read" as const)
          : messageMode === "request_reply"
            ? ("reply" as const)
            : ("delivery" as const),
      messageMode,
      canCancelReply: false,
    };
  }, [actors, messageMetaEvent]);

  const refreshMessageStatus = useCallback(
    async (eventId: string) => {
      if (!selectedGroupId || !eventId) return;
      const statusResp = await api.fetchLedgerStatuses(selectedGroupId, [eventId], {
        noCache: true,
      });
      if (statusResp.ok) {
        mergeEventStatuses(statusResp.result.statuses || {}, selectedGroupId);
      }
    },
    [mergeEventStatuses, selectedGroupId],
  );

  const handleDeliverMessage = useCallback(
    async (actorId: string, forceAmbiguous: boolean) => {
      const targetGroupId = String(selectedGroupId || "").trim();
      const eventId = String(messageMeta?.sourceEventId || "").trim();
      if (!targetGroupId || !eventId || !actorId) return;
      setMessageActionBusy(`deliver:${actorId}`);
      try {
        const resp = await api.deliverMessage(targetGroupId, eventId, [actorId], forceAmbiguous);
        if (!resp.ok) {
          if (useGroupStore.getState().selectedGroupId === targetGroupId) {
            showError(`${resp.error.code}: ${resp.error.message}`);
          }
          return;
        }
        await refreshMessageStatus(eventId);
      } finally {
        setMessageActionBusy("");
      }
    },
    [messageMeta?.sourceEventId, refreshMessageStatus, selectedGroupId, showError],
  );

  const handleCancelReplyRequest = useCallback(async () => {
    const targetGroupId = String(selectedGroupId || "").trim();
    const eventId = String(messageMeta?.sourceEventId || "").trim();
    if (!targetGroupId || !eventId) return;
    setMessageActionBusy("cancel-reply");
    try {
      const resp = await api.cancelReplyRequest(targetGroupId, eventId);
      if (!resp.ok) {
        if (useGroupStore.getState().selectedGroupId === targetGroupId) {
          showError(`${resp.error.code}: ${resp.error.message}`);
        }
        return;
      }
      await refreshMessageStatus(eventId);
    } finally {
      setMessageActionBusy("");
    }
  }, [messageMeta?.sourceEventId, refreshMessageStatus, selectedGroupId, showError]);

  // Handlers
  const handleUpdateSettings = async (settings: Partial<GroupSettings>): Promise<boolean> => {
    const targetGroupId = String(selectedGroupId || "").trim();
    if (!targetGroupId) return false;
    setBusy("settings-update");
    try {
      const resp = await api.updateSettings(targetGroupId, settings);
      if (!resp.ok) {
        throw new Error(formatGroupSettingsUpdateError(t, resp.error));
      }
      await refreshSettings(targetGroupId);
      return true;
    } finally {
      setBusy("");
    }
  };

  const handleMarkAllRead = async () => {
    if (!inboxTarget || inboxTarget.groupId !== selectedGroupId) return;
    const { groupId, actorId } = inboxTarget;
    if (inboxMessages.length === 0) return;
    const busyKey = `inbox-read:${groupId}:${actorId}`;
    setBusy(busyKey);
    try {
      const resp = await api.readInbox(groupId, actorId, inboxMessages.length);
      if (!resp.ok) {
        if (useInboxStore.getState().inboxTarget === inboxTarget) {
          showError(`${resp.error.code}: ${resp.error.message}`);
        }
        return;
      }
      const [inboxResp] = await Promise.all([
        api.fetchInbox(groupId, actorId),
        refreshActors(groupId, { includeUnread: true }),
      ]);
      if (inboxResp.ok) {
        setInboxMessages(inboxTarget, inboxResp.result.messages || []);
      }
    } finally {
      if (useUIStore.getState().busy === busyKey) setBusy("");
    }
  };

  const handleSaveGroupEdit = async () => {
    const targetGroupId = String(selectedGroupId || "").trim();
    if (!targetGroupId) return;
    setBusy("group-update");
    try {
      const resp = await api.updateGroup(targetGroupId, editGroupTitle, editGroupTopic);
      if (!resp.ok) {
        if (useGroupStore.getState().selectedGroupId === targetGroupId) {
          showError(`${resp.error.code}: ${resp.error.message}`);
        }
        return;
      }
      if (useGroupStore.getState().selectedGroupId === targetGroupId) {
        closeModal("groupEdit");
      }
      await refreshGroups();
      if (useGroupStore.getState().selectedGroupId === targetGroupId) {
        await loadGroup(targetGroupId);
      }
    } finally {
      setBusy("");
    }
  };

  const handleDeleteGroup = () => onDeleteGroup(selectedGroupId);

  const handleResetGroup = async () => {
    if (!selectedGroupId) return;
    if (!window.confirm(t("resetGroupConfirm", { name: groupDoc?.title || selectedGroupId })))
      return;
    const oldGroupId = selectedGroupId;
    setBusy("group-reset");
    try {
      const resp = await api.resetGroup(oldGroupId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const result = resp.result || {};
      const newGroupId = String(result.new_group_id || result.group_id || "").trim();
      if (!newGroupId) {
        showError("group_reset_failed: missing new_group_id");
        return;
      }
      closeModal("groupEdit");
      useGroupStore.getState().setEvents([]);
      useGroupStore.getState().setActors([]);
      setGroupContext(null);
      setGroupSettings(null);
      await refreshGroups();
      setSelectedGroupId(newGroupId);
      await loadGroup(newGroupId);
      if (result.deleted_old === false) {
        showNotice({ message: t("resetGroupOldDeleteFailed") });
      }
    } finally {
      setBusy("");
    }
  };

  const handleCreateGroup = async () => {
    const path = createGroupPath.trim();
    if (!path) return;
    const dirName = path.split("/").filter(Boolean).pop() || "working-group";
    const title = createGroupName.trim() || dirName;
    setBusy("create");
    directoryBrowser.setError("");
    try {
      const resp = await api.createGroupWithScope(title, path);
      if (!resp.ok) {
        directoryBrowser.setError(`${resp.error.code}: ${resp.error.message}`);
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const groupId = resp.result.group_id;
      resetCreateGroupForm();
      closeModal("createGroup");
      await refreshGroups();
      requestWorkspaceNavigation(() => setSelectedGroupId(groupId));
    } finally {
      setBusy("");
    }
  };

  const relaySourceGroupId = useMemo(() => {
    const fromStore = relaySource?.groupId ? String(relaySource.groupId) : "";
    if (fromStore.trim()) return fromStore.trim();
    return String(selectedGroupId || "").trim();
  }, [relaySource, selectedGroupId]);

  const relaySourceEvent = useMemo(() => {
    if (relaySource?.event) return relaySource.event;
    const eid = String(relayEventId || "").trim();
    if (!eid) return null;
    const fromWindow =
      chatWindow && String(chatWindow.groupId || "") === String(selectedGroupId || "")
        ? (chatWindow.events || []).find((ev) => String(ev.id || "") === eid) || null
        : null;
    if (fromWindow) return fromWindow;
    const fromLive = (events || []).find((ev) => String(ev.id || "") === eid) || null;
    if (fromLive) return fromLive;
    return (senderHistoryEvents || []).find((ev) => String(ev.id || "") === eid) || null;
  }, [chatWindow, events, relayEventId, relaySource, selectedGroupId, senderHistoryEvents]);

  const presentationReferenceEvents = useMemo(() => {
    const next = new Map<string, LedgerEvent>();
    for (const event of events || []) {
      if (!event?.id) continue;
      next.set(String(event.id), event);
    }
    for (const event of senderHistoryEvents || []) {
      if (!event?.id) continue;
      next.set(String(event.id), event);
    }
    if (chatWindow?.groupId === selectedGroupId) {
      for (const event of chatWindow.events || []) {
        if (!event?.id) continue;
        next.set(String(event.id), event);
      }
    }
    return Array.from(next.values());
  }, [chatWindow, events, selectedGroupId, senderHistoryEvents]);

  const presentationViewerSourceEvent = useMemo(() => {
    const focusEventId = String(presentationViewer?.focusEventId || "").trim();
    if (!focusEventId || presentationViewer?.groupId !== selectedGroupId) return null;
    return (
      presentationReferenceEvents.find((event) => String(event.id || "").trim() === focusEventId) ||
      null
    );
  }, [presentationReferenceEvents, presentationViewer, selectedGroupId]);

  const handleRelayMessage = async (dstGroupId: string, toTokens: string[], note: string) => {
    const src = relaySourceEvent;
    const srcGroupId = relaySourceGroupId;
    const dstGroup = String(dstGroupId || "").trim();
    const srcEventId = src?.id ? String(src.id) : "";
    if (!src || !srcGroupId || !srcEventId) return;
    if (!dstGroup) return;
    if (dstGroup === srcGroupId) {
      showError(t("destGroupDifferent"));
      return;
    }

    const d = src.data as ChatMessageData | undefined;
    const srcText = typeof d?.text === "string" ? d.text : "";
    const srcInsight = getMessageInsight(d);
    const srcQuoteText = typeof d?.quote_text === "string" ? d.quote_text.trim() : "";
    const noteText = String(note || "").trim();
    const relayBody = [noteText, String(srcText || "").trim()].filter(Boolean).join("\n\n");
    const relayText = appendQuotedOriginalPerspective(relayBody, srcInsight, src.by || "sender");
    if (!relayText.trim()) {
      showError(t("relayTextEmpty"));
      return;
    }

    const to = (toTokens || []).map((t) => String(t || "").trim()).filter((t) => t);

    setBusy("relay");
    try {
      const resp = await api.relayMessage(
        dstGroup,
        relayText,
        to,
        { groupId: srcGroupId, eventId: srcEventId },
        srcQuoteText,
      );
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setRelayModal(null);
      await refreshGroups();
    } finally {
      setBusy("");
    }
  };

  const handlePresentationPublishUrl = useCallback(
    async (payload: { slotId: string; url: string; title: string; summary: string }) => {
      const gid = String(selectedGroupId || "").trim();
      if (!gid) return;
      setBusy("presentation-pin");
      try {
        const resp = await api.publishPresentationUrl(gid, payload);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setGroupPresentation(resp.result.presentation);
        setPresentationPin(null);
        if (preferredPresentationSurface === "split") {
          setChatPresentationDockOpen(gid, true);
        }
        setPresentationViewer({
          groupId: gid,
          slotId: resp.result.slot_id || payload.slotId,
          surface: preferredPresentationSurface,
        });
      } finally {
        setBusy("");
      }
    },
    [
      preferredPresentationSurface,
      selectedGroupId,
      setBusy,
      setChatPresentationDockOpen,
      setGroupPresentation,
      setPresentationPin,
      setPresentationViewer,
      showError,
    ],
  );

  const handlePresentationPublishFile = useCallback(
    async (payload: { slotId: string; file: File; title: string; summary: string }) => {
      const gid = String(selectedGroupId || "").trim();
      if (!gid) return;
      setBusy("presentation-pin");
      try {
        const resp = await api.publishPresentationUpload(gid, payload);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setGroupPresentation(resp.result.presentation);
        setPresentationPin(null);
        if (preferredPresentationSurface === "split") {
          setChatPresentationDockOpen(gid, true);
        }
        setPresentationViewer({
          groupId: gid,
          slotId: resp.result.slot_id || payload.slotId,
          surface: preferredPresentationSurface,
        });
      } finally {
        setBusy("");
      }
    },
    [
      preferredPresentationSurface,
      selectedGroupId,
      setBusy,
      setChatPresentationDockOpen,
      setGroupPresentation,
      setPresentationPin,
      setPresentationViewer,
      showError,
    ],
  );

  const handlePresentationPublishWorkspace = useCallback(
    async (payload: { slotId: string; path: string; title: string; summary: string }) => {
      const gid = String(selectedGroupId || "").trim();
      if (!gid) return;
      setBusy("presentation-pin");
      try {
        const resp = await api.publishPresentationWorkspace(gid, payload);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setGroupPresentation(resp.result.presentation);
        setPresentationPin(null);
        if (preferredPresentationSurface === "split") {
          setChatPresentationDockOpen(gid, true);
        }
        setPresentationViewer({
          groupId: gid,
          slotId: resp.result.slot_id || payload.slotId,
          surface: preferredPresentationSurface,
        });
      } finally {
        setBusy("");
      }
    },
    [
      preferredPresentationSurface,
      selectedGroupId,
      setBusy,
      setChatPresentationDockOpen,
      setGroupPresentation,
      setPresentationPin,
      setPresentationViewer,
      showError,
    ],
  );

  const handlePresentationClear = useCallback(
    async (slotId: string) => {
      const gid = String(selectedGroupId || "").trim();
      const normalizedSlotId = String(slotId || "").trim();
      if (!gid || !normalizedSlotId) return;
      const confirmed = window.confirm(
        t("chat:presentationClearConfirm", {
          index: Number(normalizedSlotId.replace("slot-", "") || 0) || normalizedSlotId,
          defaultValue: `Clear ${normalizedSlotId}?`,
        }),
      );
      if (!confirmed) return;
      setBusy("presentation-clear");
      try {
        const resp = await api.clearPresentationSlot(gid, normalizedSlotId);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        setGroupPresentation(resp.result.presentation);
        setPresentationViewer(null);
        setPresentationPin(null);
        clearPresentationSlotAttention(gid, normalizedSlotId);
        forgetPresentationViewerSlot(gid, normalizedSlotId);
      } finally {
        setBusy("");
      }
    },
    [
      clearPresentationSlotAttention,
      forgetPresentationViewerSlot,
      selectedGroupId,
      setBusy,
      setGroupPresentation,
      setPresentationViewer,
      setPresentationPin,
      showError,
      t,
    ],
  );

  const handleQuotePresentationReference = useCallback(
    (payload: { slotId: string; ref?: PresentationMessageRef | null }) => {
      const gid = String(selectedGroupId || "").trim();
      const normalizedSlotId = String(payload.slotId || "").trim();
      if (!gid || !normalizedSlotId) return;
      const slot = findPresentationSlot(groupPresentation, normalizedSlotId);
      const ref = payload.ref || buildPresentationRefForSlot(slot);
      if (!ref) {
        showError(
          t("chat:presentationMissingCard", { defaultValue: "This presentation slot is empty." }),
        );
        return;
      }
      setQuotedPresentationRef(ref);
      setComposerDestGroupId(gid);
      setActiveTab("chat");
      setChatMobileSurface(gid, "messages");
      setPresentationViewer(null);
      window.setTimeout(() => composerRef.current?.focus(), 0);
    },
    [
      composerRef,
      groupPresentation,
      selectedGroupId,
      setActiveTab,
      setChatMobileSurface,
      setComposerDestGroupId,
      setPresentationViewer,
      setQuotedPresentationRef,
      showError,
      t,
    ],
  );

  const handleOpenPresentationMessageContext = useCallback(
    async (eventId: string) => {
      const gid = String(selectedGroupId || "").trim();
      const eid = String(eventId || "").trim();
      if (!gid || !eid) return;
      setActiveTab("chat");
      setChatMobileSurface(gid, "messages");
      setPresentationViewer(null);
      await openChatWindow(gid, eid);
    },
    [openChatWindow, selectedGroupId, setActiveTab, setChatMobileSurface, setPresentationViewer],
  );

  const handleReplyToPresentationMessage = useCallback(
    async (event: LedgerEvent) => {
      const gid = String(selectedGroupId || "").trim();
      const eid = String(event.id || "").trim();
      if (!gid || !eid) return;
      setActiveTab("chat");
      setChatMobileSurface(gid, "messages");
      onStartReply(event);
      setPresentationViewer(null);
      await openChatWindow(gid, eid);
      window.setTimeout(() => composerRef.current?.focus(), 0);
    },
    [
      composerRef,
      onStartReply,
      openChatWindow,
      selectedGroupId,
      setActiveTab,
      setChatMobileSurface,
      setPresentationViewer,
    ],
  );

  return (
    <>
      <MobileMenuSheet
        isOpen={modals.mobileMenu}
        theme={theme}
        textScale={textScale}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        selectedGroupRunning={selectedGroupRunning}
        onClose={() => closeModal("mobileMenu")}
        onOpenFiles={
          isSmallScreen && selectedGroupId
            ? () => setChatMobileSurface(selectedGroupId, "files")
            : undefined
        }
        onThemeChange={onThemeChange}
        onTextScaleChange={onTextScaleChange}
        onOpenSearch={() => openModal("search")}
        onOpenContext={() => {
          openModal("context");
        }}
        onOpenSettings={() => openModal("settings")}
        canAccessAccount={canManageGroups}
        accountLabel={accountLabel}
        onOpenAccount={() => openSettingsTarget({ scope: "global", tab: "account" })}
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
      />

      {modals.relay && relayEventId ? (
        <RelayMessageModal
          key={`${selectedGroupId}:${relayEventId}`}
          isOpen={true}
          busy={busy === "relay"}
          srcGroupId={relaySourceGroupId}
          srcEvent={relaySourceEvent}
          groups={groups}
          onCancel={() => setRelayModal(null)}
          onSubmit={(dstGroupId, to, note) => void handleRelayMessage(dstGroupId, to, note)}
        />
      ) : null}

      <SearchModal
        groupTitle={groupDoc?.title}
        isOpen={modals.search}
        onClose={() => closeModal("search")}
        groupId={selectedGroupId}
        actors={actors}
        isDark={isDark}
        onReply={(ev) => {
          onStartReply(ev);
          setActiveTab("chat");
          closeModal("search");
          window.setTimeout(() => composerRef.current?.focus(), 0);
        }}
        onJumpToMessage={(eventId) => {
          const gid = String(selectedGroupId || "").trim();
          const eid = String(eventId || "").trim();
          if (!gid || !eid) return;
          setActiveTab("chat");
          closeModal("search");
          const url = new URL(window.location.href);
          url.searchParams.set("group", gid);
          url.searchParams.set("event", eid);
          url.searchParams.set("tab", "chat");
          window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());
          void openChatWindow(gid, eid);
        }}
      />

      <PresentationPinModal
        key={
          presentationPin
            ? `${presentationPin.groupId}:${presentationPin.slotId}:${
                findPresentationSlot(groupPresentation, presentationPin?.slotId || "")?.card
                  ?.published_at || "empty"
              }:${presentationPin.workspacePath || ""}`
            : "presentation-pin-closed"
        }
        isOpen={!!presentationPin && presentationPin.groupId === selectedGroupId}
        groupId={selectedGroupId}
        isDark={isDark}
        slot={
          presentationPin?.groupId === selectedGroupId
            ? findPresentationSlot(groupPresentation, presentationPin?.slotId || "")
            : null
        }
        initialWorkspaceRelPath={presentationPin?.workspacePath || ""}
        busy={busy === "presentation-pin"}
        onClose={() => setPresentationPin(null)}
        onSubmitUrl={handlePresentationPublishUrl}
        onSubmitWorkspace={handlePresentationPublishWorkspace}
        onSubmitFile={handlePresentationPublishFile}
      />

      {presentationViewerSlotIds.length > 0 ? (
        <Suspense fallback={<LazyModalFallback isDark={isDark} />}>
          {presentationViewerSlotIds.map((slotId) => {
            const slot = findPresentationSlot(groupPresentation, slotId);
            const version = String(slot?.card?.published_at || "empty").trim() || "empty";
            return (
              <PresentationViewerModal
                key={`${selectedGroupId}:${slotId}:${version}`}
                isOpen={
                  !!presentationViewer &&
                  presentationViewer.surface !== "split" &&
                  presentationViewer.groupId === selectedGroupId &&
                  presentationViewer.slotId === slotId
                }
                isDark={isDark}
                readOnly={readOnly}
                groupId={selectedGroupId}
                slotId={slotId}
                presentation={groupPresentation}
                focusRef={
                  presentationViewer?.groupId === selectedGroupId &&
                  presentationViewer.slotId === slotId
                    ? presentationViewer.focusRef || null
                    : null
                }
                focusEventId={
                  presentationViewer?.groupId === selectedGroupId &&
                  presentationViewer.slotId === slotId
                    ? presentationViewer.focusEventId || null
                    : null
                }
                sourceEvent={
                  presentationViewer?.groupId === selectedGroupId &&
                  presentationViewer.slotId === slotId
                    ? presentationViewerSourceEvent
                    : null
                }
                onSelectSlot={(nextSlotId) =>
                  setPresentationViewer({
                    groupId: selectedGroupId,
                    slotId: nextSlotId,
                    surface: "modal",
                  })
                }
                onPinSlot={(nextSlotId) => {
                  setPresentationViewer(null);
                  setPresentationPin({ groupId: selectedGroupId, slotId: nextSlotId });
                }}
                onQuoteInChat={handleQuotePresentationReference}
                onOpenMessageContext={(eventId) =>
                  void handleOpenPresentationMessageContext(eventId)
                }
                onReplyToMessage={(event) => void handleReplyToPresentationMessage(event)}
                onReplaceSlot={(nextSlotId) => {
                  const gid = String(selectedGroupId || "").trim();
                  if (!gid || !nextSlotId) return;
                  setPresentationViewer(null);
                  setPresentationPin({ groupId: gid, slotId: nextSlotId });
                }}
                onClearSlot={(nextSlotId) => void handlePresentationClear(nextSlotId)}
                supportsSplit={!isSmallScreen}
                onOpenSplit={() => {
                  const gid = String(selectedGroupId || "").trim();
                  if (!gid || !presentationViewer) return;
                  setChatPresentationDisplayMode(gid, "split");
                  setChatPresentationDockOpen(gid, true);
                  setPresentationViewer({ ...presentationViewer, surface: "split" });
                }}
                onClose={() => setPresentationViewer(null)}
              />
            );
          })}
        </Suspense>
      ) : null}

      {modals.context ? (
        <Suspense fallback={<LazyModalFallback isDark={isDark} />}>
          <ContextModal
            isOpen={modals.context}
            onClose={() => closeModal("context")}
            groupId={selectedGroupId}
            groupTitle={groupDoc?.group_id === selectedGroupId ? groupDoc.title : undefined}
            context={groupContext}
            initialTaskId={contextTaskId}
            onInitialTaskHandled={clearContextTask}
            onOpenContext={() => openContextModalData(fetchContext, selectedGroupId)}
            onSyncContext={() => syncContextModalData(fetchContext, selectedGroupId)}
            isDark={isDark}
          />
        </Suspense>
      ) : null}

      {modals.settings ? (
        <Suspense fallback={<LazyModalFallback isDark={isDark} />}>
          <SettingsModal
            codexVoice={codexVoice}
            isOpen={modals.settings}
            onClose={() => closeModal("settings")}
            settings={groupSettings}
            onUpdateSettings={handleUpdateSettings}
            onRegistryChanged={refreshGroups}
            busy={busy.startsWith("settings")}
            isDark={isDark}
            groupId={selectedGroupId}
            groupDoc={groupDoc}
          />
        </Suspense>
      ) : null}

      <RecipientsModal
        isOpen={!!messageMeta}
        isDark={isDark}
        toLabel={messageMeta?.toLabel || ""}
        statusKind={messageMeta?.statusKind || "read"}
        entries={messageMeta?.entries || []}
        messageMode={messageMeta?.messageMode || "send"}
        busyAction={messageActionBusy}
        remoteDelivery={
          messageMeta && "remoteDelivery" in messageMeta ? messageMeta.remoteDelivery : undefined
        }
        remoteCancellation={messageMetaEvent?._connect_cancellation}
        canCancelReply={Boolean(messageMeta?.canCancelReply)}
        onDeliver={(actorId, forceAmbiguous) => {
          void handleDeliverMessage(actorId, forceAmbiguous);
        }}
        onCancelReply={() => {
          void handleCancelReplyRequest();
        }}
        onClose={() => setRecipientsModal(null)}
      />

      <InboxModal
        isOpen={modals.inbox && inboxTarget?.groupId === selectedGroupId}
        isDark={isDark}
        actorId={inboxTarget?.actorId || ""}
        actors={actors}
        messages={inboxMessages}
        busy={busy}
        onClose={() => {
          clearInbox();
          closeModal("inbox");
        }}
        onMarkAllRead={handleMarkAllRead}
      />

      <GroupEditModal
        isOpen={modals.groupEdit}
        isDark={isDark}
        busy={busy}
        groupId={selectedGroupId || groupDoc?.group_id || ""}
        ccccHome={ccccHome}
        projectRoot={(() => {
          const key = String(groupDoc?.active_scope_key || "").trim();
          const scopes = Array.isArray(groupDoc?.scopes) ? groupDoc?.scopes : [];
          const active = scopes.find((s) => String(s?.scope_key || "").trim() === key);
          const url = String(active?.url || scopes[0]?.url || "").trim();
          return url;
        })()}
        title={editGroupTitle}
        topic={editGroupTopic}
        onChangeTitle={setEditGroupTitle}
        onChangeTopic={setEditGroupTopic}
        onSave={handleSaveGroupEdit}
        onCancel={() => closeModal("groupEdit")}
        onReset={handleResetGroup}
        onDelete={handleDeleteGroup}
      />

      <ActorConfigModal
        mode="edit"
        initialSection={editingActorSection}
        isOpen={!!editingActor}
        isDark={isDark}
        busy={busy}
        groupId={selectedGroupId || groupDoc?.group_id || ""}
        scopes={groupDoc?.group_id === selectedGroupId ? groupDoc.scopes : undefined}
        activeScopeKey={
          groupDoc?.group_id === selectedGroupId ? groupDoc.active_scope_key : undefined
        }
        onScopeAttached={
          groupDoc?.group_id === selectedGroupId ? handleActorScopeAttached : undefined
        }
        actorId={editingActor?.id || ""}
        groupRole={editingActor?.role || "peer"}
        avatarUrl={editingActor?.avatar_url || undefined}
        hasCustomAvatar={!!editingActor?.has_custom_avatar}
        isRunning={!!(editingActor && (editingActor.running ?? editingActor.enabled ?? false))}
        savedRuntime={editingActor?.runtime || "codex"}
        savedActor={editingActor || undefined}
        savedActorNotes={savedActorNotes}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        title={editActorTitle}
        onChangeTitle={setEditActorTitle}
        actorNotes={editActorNotes}
        onChangeActorNotes={setEditActorNotes}
        actorNotesBusy={editActorNotesBusy}
        capabilityAutoloadText={editActorCapabilityAutoloadText}
        onChangeCapabilityAutoloadText={setEditActorCapabilityAutoloadText}
        onSave={handleSaveEditActorOnly}
        onSaveAndRestart={handleSaveEditActorAndRestart}
        linkedProfileId={String(editingActor?.profile_id || "") || undefined}
        linkedProfileScope={
          (String(editingActor?.profile_scope || "global").trim() || "global") as "global" | "user"
        }
        linkedProfileOwner={String(editingActor?.profile_owner || "").trim() || undefined}
        actorProfiles={actorProfiles}
        actorProfilesBusy={actorProfilesBusy}
        onRequestActorProfiles={loadActorProfiles}
        onSaveAsProfile={handleSaveEditActorAsProfile}
        onAvatarChanged={refreshActors}
        onCancel={handleCancelEditActor}
      />

      <CreateGroupModal
        isOpen={modals.createGroup}
        isDark={isDark}
        busy={busy}
        dirSuggestions={dirSuggestions}
        dirItems={dirItems}
        currentDir={currentDir}
        parentDir={parentDir}
        showDirBrowser={showDirBrowser}
        createGroupPath={createGroupPath}
        setCreateGroupPath={setCreateGroupPath}
        createGroupName={createGroupName}
        setCreateGroupName={setCreateGroupName}
        dirBrowseError={directoryBrowser.error}
        creatingDirectory={directoryBrowser.creating}
        onFetchDirContents={directoryBrowser.fetchContents}
        onCreateDirectory={directoryBrowser.createDirectory}
        onCreateGroup={handleCreateGroup}
        onClose={() => closeModal("createGroup")}
        onCancelAndReset={() => {
          closeModal("createGroup");
          resetCreateGroupForm();
          directoryBrowser.setError("");
        }}
      />

      <ActorConfigModal
        key={`create-actor-${selectedGroupId}`}
        mode="create"
        groupId={selectedGroupId || groupDoc?.group_id || ""}
        scopes={groupDoc?.group_id === selectedGroupId ? groupDoc.scopes : undefined}
        activeScopeKey={
          groupDoc?.group_id === selectedGroupId ? groupDoc.active_scope_key : undefined
        }
        onScopeAttached={
          groupDoc?.group_id === selectedGroupId ? handleActorScopeAttached : undefined
        }
        isOpen={modals.addActor}
        isDark={isDark}
        busy={busy}
        hasForeman={hasForeman}
        runtimes={runtimes}
        suggestedActorId={suggestedActorId}
        actorId={newActorId}
        onChangeActorId={setNewActorId}
        role={newActorRole}
        onChangeRole={setNewActorRole}
        useProfile={newActorUseProfile}
        onChangeUseProfile={setNewActorUseProfile}
        profileId={newActorProfileId}
        onChangeProfileId={setNewActorProfileId}
        actorProfiles={actorProfiles}
        actorProfilesBusy={actorProfilesBusy}
        onRequestActorProfiles={loadActorProfiles}
        runtime={newActorRuntime}
        onChangeRuntime={setNewActorRuntime}
        command={newActorCommand}
        onChangeCommand={setNewActorCommand}
        useDefaultCommand={newActorUseDefaultCommand}
        onChangeUseDefaultCommand={setNewActorUseDefaultCommand}
        secretsSetText={newActorSecretsSetText}
        onChangeSecretsSetText={setNewActorSecretsSetText}
        capabilityAutoloadText={newActorCapabilityAutoloadText}
        onChangeCapabilityAutoloadText={setNewActorCapabilityAutoloadText}
        actorNotes={newActorNotes}
        onChangeActorNotes={setNewActorNotes}
        error={addActorError}
        onChangeError={setAddActorError}
        canSubmit={canAddActor}
        submitDisabledReason={addActorDisabledReason}
        onCreate={handleAddActor}
        onSaveAsProfile={handleSaveNewActorAsProfile}
        onCancel={handleCloseAddActor}
      />
    </>
  );
}
