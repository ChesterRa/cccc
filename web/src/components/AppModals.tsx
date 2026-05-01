// AppModals renders all modal components in one place.
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SearchModal } from "./SearchModal";
import type { TemplatePreviewDetailsProps } from "./TemplatePreviewDetails";
import { MobileMenuSheet } from "./layout/MobileMenuSheet";
import { AddActorModal } from "./modals/AddActorModal";
import { CreateGroupModal } from "./modals/CreateGroupModal";
import {
  EditActorModal,
  NO_CHANGES_SENTINEL,
  type EditActorSavePayload,
  type SaveActorProfileResult,
} from "./modals/EditActorModal";
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
import { parsePrivateEnvSetText } from "../utils/privateEnvInput";
import { parseHelpMarkdown, updateActorHelpNote } from "../utils/helpMarkdown";
import { formatCapabilityIdInput, normalizeCapabilityIdList, parseCapabilityIdInput } from "../utils/capabilityAutoload";
import { actorProfileIdentityKey, actorProfileMatchesRef } from "../utils/actorProfiles";
import { findPresentationSlot } from "../utils/presentation";
import { buildPresentationRefForSlot } from "../utils/presentationRefs";
import { formatGroupSettingsUpdateError } from "../utils/groupSettingsErrors";
import { getEffectiveActorRunner, normalizeActorRunner } from "../utils/headlessRuntimeSupport";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useComposerStore,
  useInboxStore,
  useFormStore,
} from "../stores";
import { getAckRecipientIdsForEvent, getRecipientActorIdsForEvent } from "../hooks/useSSE";
import { getChatSession } from "../stores/useUIStore";
import * as api from "../services/api";
import { Actor, ActorProfile, RUNTIME_INFO, LedgerEvent, GroupSettings, ChatMessageData, PresentationMessageRef, SupportedRuntime, TextScale, Theme } from "../types";

const ContextModal = lazy(() => import("./ContextModal/index").then((module) => ({ default: module.ContextModal })));
const SettingsModal = lazy(() => import("./SettingsModal").then((module) => ({ default: module.SettingsModal })));
const PresentationViewerModal = lazy(() =>
  import("./presentation/PresentationViewerModal").then((module) => ({ default: module.PresentationViewerModal }))
);

interface AppModalsProps {
  isDark: boolean;
  theme: Theme;
  textScale: TextScale;
  readOnly?: boolean;
  ccccHome: string;
  composerRef: React.RefObject<HTMLTextAreaElement | null>;
  onStartReply: (ev: LedgerEvent) => void;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  onStartGroup: () => Promise<void>;
  onStopGroup: () => Promise<void>;
  onSetGroupState: (state: "active" | "idle" | "paused") => Promise<void>;
  fetchContext: ContextModalFetch;
  canManageGroups: boolean;
}

function getErrorDetailGroupId(err: unknown): string {
  if (!err || typeof err !== "object") return "";
  const details = (err as { details?: unknown }).details;
  if (!details || typeof details !== "object") return "";
  return String((details as { group_id?: unknown }).group_id || "").trim();
}

function sortPresentationSlotIds(slotIds: string[]): string[] {
  return [...slotIds].sort((left, right) => {
    const leftIndex = Number(String(left || "").replace("slot-", "")) || 0;
    const rightIndex = Number(String(right || "").replace("slot-", "")) || 0;
    return leftIndex - rightIndex;
  });
}

function LazyModalFallback({ isDark }: { isDark: boolean }) {
  return (
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/40 backdrop-blur-[1px]">
      <div
        className={[
          "rounded-2xl border px-4 py-3 text-sm shadow-xl",
          isDark ? "border-slate-700 bg-slate-900 text-slate-100" : "border-slate-200 bg-white text-slate-700",
        ].join(" ")}
      >
        Loading...
      </div>
    </div>
  );
}

export function AppModals({
  isDark,
  theme,
  textScale,
  readOnly,
  ccccHome,
  composerRef,
  onStartReply,
  onThemeChange,
  onTextScaleChange,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  fetchContext,
  canManageGroups,
}: AppModalsProps) {
  const { t } = useTranslation(['actors', 'chat', 'modals']);
  // Stores
  const {
    groups,
    selectedGroupId,
    groupDoc,
    events,
    chatWindow,
    actors,
    groupContext,
    groupSettings,
    groupPresentation,
    runtimes,
    setSelectedGroupId,
    setGroupDoc,
    setGroupContext,
    setGroupSettings,
    setGroupPresentation,
    refreshGroups,
    refreshSettings,
    refreshActors,
    loadGroup,
    openChatWindow,
  } = useGroupStore();

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
  } = useUIStore();

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
  } = useModalStore();
  const contextTaskId = useModalStore((state) => state.contextTaskId);

  const { inboxActorId, inboxMessages, setInboxMessages } = useInboxStore();
  const setQuotedPresentationRef = useComposerStore((state) => state.setQuotedPresentationRef);
  const setComposerDestGroupId = useComposerStore((state) => state.setDestGroupId);

  const preferredPresentationSurface = selectedGroupId
    ? (!isSmallScreen && getChatSession(selectedGroupId, chatSessions).presentationDisplayMode === "split" ? "split" : "modal")
    : "modal";

  const {
    editGroupTitle,
    editGroupTopic,
    setEditGroupTitle,
    setEditGroupTopic,
    editActorRuntime,
    editActorRunner,
    editActorCommand,
    editActorTitle,
    editActorRoleNotes,
    editActorCapabilityAutoloadText,
    setEditActorRuntime,
    setEditActorRunner,
    setEditActorCommand,
    setEditActorTitle,
    setEditActorRoleNotes,
    setEditActorCapabilityAutoloadText,
    newActorId,
    newActorRole,
    newActorRuntime,
    newActorRunner,
    newActorCommand,
    newActorUseDefaultCommand,
    newActorSecretsSetText,
    newActorCapabilityAutoloadText,
    newActorRoleNotes,
    newActorUseProfile,
    newActorProfileId,
    showAdvancedActor,
    addActorError,
    setNewActorId,
    setNewActorRole,
    setNewActorRuntime,
    setNewActorRunner,
    setNewActorCommand,
    setNewActorUseDefaultCommand,
    setNewActorSecretsSetText,
    setNewActorCapabilityAutoloadText,
    setNewActorRoleNotes,
    setNewActorUseProfile,
    setNewActorProfileId,
    setShowAdvancedActor,
    setAddActorError,
    resetAddActorForm,
    createGroupPath,
    createGroupName,
    createGroupTemplateFile,
    dirItems,
    dirSuggestions,
    currentDir,
    parentDir,
    showDirBrowser,
    setCreateGroupPath,
    setCreateGroupName,
    setCreateGroupTemplateFile,
    setDirItems,
    setCurrentDir,
    setParentDir,
    setShowDirBrowser,
    resetCreateGroupForm,
  } = useFormStore();

  const [createTemplatePreview, setCreateTemplatePreview] = useState<TemplatePreviewDetailsProps["template"] | null>(null);
  const [createTemplateError, setCreateTemplateError] = useState("");
  const [createTemplateBusy, setCreateTemplateBusy] = useState(false);
  const [dirBrowseError, setDirBrowseError] = useState("");
  const [actorProfiles, setActorProfiles] = useState<ActorProfile[]>([]);
  const [actorProfilesBusy, setActorProfilesBusy] = useState(false);
  const [editActorRoleNotesBusy, setEditActorRoleNotesBusy] = useState(false);
  const [presentationViewerCacheByGroup, setPresentationViewerCacheByGroup] = useState<Record<string, string[]>>({});
  const editActorRoleNotesBaselineRef = useRef("");
  const editActorRoleNotesSeqRef = useRef(0);

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
      return {
        ...current,
        [normalizedGroupId]: nextSlots,
      };
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
      (slotId) => !!findPresentationSlot(groupPresentation, slotId)?.card
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

  const loadEditingActorRoleNotes = useCallback(async (groupId: string, actorId: string) => {
    const gid = String(groupId || "").trim();
    const aid = String(actorId || "").trim();
    if (!gid || !aid) {
      editActorRoleNotesBaselineRef.current = "";
      setEditActorRoleNotes("");
      return;
    }
    const seq = ++editActorRoleNotesSeqRef.current;
    setEditActorRoleNotesBusy(true);
    try {
      const resp = await api.fetchGroupPrompts(gid);
      if (!resp.ok) {
        if (seq === editActorRoleNotesSeqRef.current) {
          editActorRoleNotesBaselineRef.current = "";
          setEditActorRoleNotes("");
        }
        return;
      }
      const helpContent = String(resp.result?.help?.content || "");
      const parsed = parseHelpMarkdown(helpContent);
      const note = String(parsed.actorNotes[aid] || "");
      if (seq !== editActorRoleNotesSeqRef.current) return;
      editActorRoleNotesBaselineRef.current = note.trim();
      setEditActorRoleNotes(note);
    } finally {
      if (seq === editActorRoleNotesSeqRef.current) {
        setEditActorRoleNotesBusy(false);
      }
    }
  }, [setEditActorRoleNotes]);

  const persistActorRoleNotes = useCallback(
    async (groupId: string, actorId: string, note: string, actorOrder?: string[]) => {
      const gid = String(groupId || "").trim();
      const aid = String(actorId || "").trim();
      const nextNote = String(note || "").trim();
      if (!gid || !aid) return { ok: false as const, error: "missing actor or group" };

      const promptsResp = await api.fetchGroupPrompts(gid);
      if (!promptsResp.ok) {
        return {
          ok: false as const,
          error: `${promptsResp.error?.code || "prompt_fetch_failed"}: ${promptsResp.error?.message || "Failed to load help prompt"}`,
        };
      }

      const currentHelpContent = String(promptsResp.result?.help?.content || "");
      const nextHelpContent = updateActorHelpNote(currentHelpContent, aid, nextNote, actorOrder);
      const helpResp = await api.updateGroupPrompt(gid, "help", nextHelpContent, {
        editorMode: "structured",
        changedBlocks: [`actor:${aid}`],
      });
      if (!helpResp.ok) {
        return {
          ok: false as const,
          error: `${helpResp.error?.code || "prompt_save_failed"}: ${helpResp.error?.message || "Failed to save help prompt"}`,
        };
      }

      return { ok: true as const };
    },
    []
  );

  // Computed
  const selectedGroupRunning = useGroupStore(
    (s) => s.groups.find((g) => String(g.group_id || "") === s.selectedGroupId)?.running ?? false
  );
  const hasForeman = actors.some((a) => a.role === "foreman");

  // Compute messageMeta for RecipientsModal (moved from App.tsx)
  const messageMetaEvent = useMemo(() => {
    if (!_recipientsEventId) return null;
    const liveHit = events.find(
      (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId
    );
    if (liveHit) return liveHit;
    const windowEvents = Array.isArray(chatWindow?.events) ? chatWindow.events : [];
    return (
      windowEvents.find(
        (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId
      ) || null
    );
  }, [chatWindow?.events, events, _recipientsEventId]);

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
    const os =
      messageMetaEvent._obligation_status && typeof messageMetaEvent._obligation_status === "object"
        ? messageMetaEvent._obligation_status
        : null;
    if (os) {
      const recipientIds = Object.keys(os);
      const recipientIdSet = new Set(recipientIds);
      const entries = [
        ...actors
          .map((a) => String(a.id || ""))
          .filter((id) => id && recipientIdSet.has(id))
          .map((id) => [id, !!(os[id]?.reply_required ? os[id]?.replied : os[id]?.acked)] as const),
        recipientIdSet.has("user")
          ? (["user", !!(os["user"]?.reply_required ? os["user"]?.replied : os["user"]?.acked)] as const)
          : null,
      ].filter(Boolean) as Array<readonly [string, boolean]>;
      const anyReplyRequired = recipientIds.some((id) => !!os[id]?.reply_required);
      return { toLabel, entries, statusKind: anyReplyRequired ? ("reply" as const) : ("ack" as const) };
    }

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

  const loadActorProfiles = useCallback(async () => {
    setActorProfilesBusy(true);
    try {
      const resp = await api.listActorProfiles();
      if (!resp.ok) {
        showError(resp.error?.message || t("failedToLoadActorProfiles"));
        return;
      }
      setActorProfiles(Array.isArray(resp.result?.profiles) ? resp.result.profiles : []);
    } finally {
      setActorProfilesBusy(false);
    }
  }, [showError, t]);

  useEffect(() => {
    if (modals.addActor) {
      void loadActorProfiles();
      return;
    }
    const linkedProfileId = String(editingActor?.profile_id || "").trim();
    if (!linkedProfileId) return;
    void loadActorProfiles();
  }, [modals.addActor, editingActor?.profile_id, loadActorProfiles]);

  // Handlers
  const handleUpdateSettings = async (settings: Partial<GroupSettings>): Promise<boolean> => {
    if (!selectedGroupId) return false;
    setBusy("settings-update");
    try {
      const resp = await api.updateSettings(selectedGroupId, settings);
      if (!resp.ok) {
        showError(formatGroupSettingsUpdateError(t, resp.error));
        return false;
      }
      await refreshSettings(selectedGroupId);
      return true;
    } finally {
      setBusy("");
    }
  };

  const handleMarkAllRead = async () => {
    if (!selectedGroupId || !inboxActorId) return;
    const last = inboxMessages.length ? inboxMessages[inboxMessages.length - 1] : null;
    const eventId = last?.id ? String(last.id) : "";
    if (!eventId) return;
    setBusy(`inbox-read:${inboxActorId}`);
    try {
      const resp = await api.markInboxRead(selectedGroupId, inboxActorId, eventId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const inboxResp = await api.fetchInbox(selectedGroupId, inboxActorId);
      if (inboxResp.ok) {
        setInboxMessages(inboxResp.result.messages || []);
      }
    } finally {
      setBusy("");
    }
  };

  const handleSaveGroupEdit = async () => {
    if (!selectedGroupId) return;
    setBusy("group-update");
    try {
      const resp = await api.updateGroup(selectedGroupId, editGroupTitle, editGroupTopic);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      closeModal("groupEdit");
      await refreshGroups();
      await loadGroup(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleDeleteGroup = async () => {
    if (!selectedGroupId) return;
    if (!window.confirm(t('deleteGroupConfirm', { name: groupDoc?.title || selectedGroupId }))) return;
    setBusy("group-delete");
    try {
      const resp = await api.deleteGroup(selectedGroupId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setSelectedGroupId("");
      setGroupDoc(null);
      useGroupStore.getState().setEvents([]);
      useGroupStore.getState().setActors([]);
      setGroupContext(null);
      setGroupSettings(null);
      await refreshGroups();
    } finally {
      setBusy("");
    }
  };

  const handleSaveEditActor = async (
    payload: EditActorSavePayload,
    options: { restart: boolean }
  ) => {
    if (!selectedGroupId || !editingActor) return;

    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;

    const label = String(editingActor.title || editingActor.id || actorId).trim() || actorId;
    const mode = payload.mode === "profile" ? "profile" : "custom";
    const profileSelectionKey = String(payload.profileId || "").trim();
    const selectedProfile = mode === "profile"
      ? actorProfiles.find((item) => actorProfileIdentityKey(item) === profileSelectionKey) || null
      : null;
    const profileId = String(selectedProfile?.id || "").trim();
    const linkedBefore = Boolean(String(editingActor.profile_id || "").trim());
    const convertToCustom = mode === "custom" && linkedBefore && !!payload.convertToCustom;

    if (mode === "profile" && !selectedProfile) {
      showError(t("profileRequired"));
      return;
    }
    if (mode === "custom" && linkedBefore && !convertToCustom) {
      showError(t("profileControlsRuntimeFields"));
      return;
    }

    const setVars = payload?.setVars && typeof payload.setVars === "object" ? payload.setVars : {};
    const setKeys = Object.keys(setVars);
    const unsetKeys = Array.isArray(payload?.unsetKeys) ? payload.unsetKeys : [];
    const clear = !!payload?.clear;
    const canEditSecrets = mode === "custom" && (!linkedBefore || convertToCustom);
    const willChangeSecrets = canEditSecrets && (clear || setKeys.length > 0 || unsetKeys.length > 0);

    const currentRuntime = String(editingActor.runtime || "codex").trim();
    const currentRunner = getEffectiveActorRunner(editingActor);
    const currentCommand = Array.isArray(editingActor.command)
      ? editingActor.command.filter((item) => typeof item === "string" && item.trim()).join(" ").trim()
      : "";
    const currentTitle = String(editingActor.title || "").trim();
    const currentCapabilityAutoload = normalizeCapabilityIdList(
      (editingActor as { capability_autoload?: unknown[] })?.capability_autoload
    );
    const currentRoleNotes = String(editActorRoleNotesBaselineRef.current || "").trim();
    const nextRoleNotes = String(editActorRoleNotes || "").trim();
    const nextRuntime = String(editActorRuntime || "codex").trim();
    const nextRunner = normalizeActorRunner(editActorRunner);
    const nextCommand = String(editActorCommand || "").trim();
    const nextTitle = String(editActorTitle || "").trim();
    const nextCapabilityAutoload = Array.isArray(payload.capabilityAutoload)
      ? normalizeCapabilityIdList(payload.capabilityAutoload)
      : [];

    const runtimeChanged = mode === "custom" && (!linkedBefore || convertToCustom) && nextRuntime !== currentRuntime;
    const runnerChanged = mode === "custom" && (!linkedBefore || convertToCustom) && nextRunner !== currentRunner;
    const commandChanged = mode === "custom" && (!linkedBefore || convertToCustom) && nextCommand !== currentCommand;
    const titleChanged = nextTitle !== currentTitle;
    const autoloadChanged =
      JSON.stringify(nextCapabilityAutoload) !== JSON.stringify(currentCapabilityAutoload);
    const profileChanged = mode === "profile" && !actorProfileMatchesRef(selectedProfile || { id: "", scope: "global", owner_id: "" }, {
      profileId: String(editingActor.profile_id || "").trim(),
      profileScope: String(editingActor.profile_scope || "global").trim() || "global",
      profileOwner: String(editingActor.profile_owner || "").trim(),
    });
    const roleNotesChanged = nextRoleNotes !== currentRoleNotes;
    const hasActorMutation =
      convertToCustom || runtimeChanged || runnerChanged || commandChanged || titleChanged || autoloadChanged || profileChanged;

    if (!options.restart && !hasActorMutation && !willChangeSecrets && !roleNotesChanged) {
      throw new Error(NO_CHANGES_SENTINEL);
    }

    if (options.restart) {
      const msg = willChangeSecrets
        ? t("saveSecretsAndRestartConfirm", { label })
        : t("saveAndRestartConfirm", { label });
      if (!window.confirm(msg)) return;
    }

    setBusy("actor-update");
    try {
      let actorSnapshot: Record<string, unknown> = editingActor as unknown as Record<string, unknown>;

      if (mode === "custom" && linkedBefore && convertToCustom) {
        const convertResp = await api.updateActor(
          selectedGroupId,
          actorId,
          undefined,
          undefined,
          undefined,
          nextTitle,
          {
            profileAction: "convert_to_custom",
            capabilityAutoload: nextCapabilityAutoload,
          }
        );
        if (!convertResp.ok) {
          showError(`${convertResp.error.code}: ${convertResp.error.message}`);
          return;
        }
        const updated =
          convertResp.result && typeof convertResp.result === "object"
            ? (convertResp.result as { actor?: Record<string, unknown> }).actor
            : undefined;
        if (updated && typeof updated === "object") actorSnapshot = updated;
      }

      if (mode === "profile") {
        const needProfilePatch = profileChanged || titleChanged || autoloadChanged;
        if (needProfilePatch) {
          const profileResp = await api.updateActor(
            selectedGroupId,
            actorId,
            undefined,
            undefined,
            undefined,
            nextTitle,
            {
              profileId,
              profileScope: (selectedProfile?.scope || "global") as api.ProfileScope,
              profileOwner: String(selectedProfile?.owner_id || "").trim() || undefined,
              capabilityAutoload: nextCapabilityAutoload,
            }
          );
          if (!profileResp.ok) {
            showError(`${profileResp.error.code}: ${profileResp.error.message}`);
            return;
          }
          const updated =
            profileResp.result && typeof profileResp.result === "object"
              ? (profileResp.result as { actor?: Record<string, unknown> }).actor
              : undefined;
          if (updated && typeof updated === "object") actorSnapshot = updated;
        }
      } else {
        const snapshotRuntime = String(actorSnapshot.runtime || currentRuntime || "codex").trim();
        const snapshotCommand = Array.isArray(actorSnapshot.command)
          ? actorSnapshot.command.filter((item) => typeof item === "string" && item.trim()).join(" ").trim()
          : currentCommand;
        const snapshotRunner = getEffectiveActorRunner(actorSnapshot as { runner?: unknown; runner_effective?: unknown });
        const snapshotTitle = String(actorSnapshot.title || "").trim();
        const needCustomPatch =
          nextRuntime !== snapshotRuntime ||
          nextRunner !== snapshotRunner ||
          nextCommand !== snapshotCommand ||
          nextTitle !== snapshotTitle ||
          autoloadChanged;
        if (needCustomPatch) {
          const customResp = await api.updateActor(
            selectedGroupId,
            actorId,
            editActorRuntime,
            nextRunner,
            editActorCommand,
            nextTitle,
            { capabilityAutoload: nextCapabilityAutoload }
          );
          if (!customResp.ok) {
            showError(`${customResp.error.code}: ${customResp.error.message}`);
            return;
          }
          const updated =
            customResp.result && typeof customResp.result === "object"
              ? (customResp.result as { actor?: Record<string, unknown> }).actor
              : undefined;
          if (updated && typeof updated === "object") actorSnapshot = updated;
        }
      }

      if (willChangeSecrets) {
        const envResp = await api.updateActorPrivateEnv(selectedGroupId, actorId, setVars, unsetKeys, clear);
        if (!envResp.ok) {
          showError(`${envResp.error.code}: ${envResp.error.message}`);
          return;
        }
      }

      if (roleNotesChanged) {
        const roleNotesResp = await persistActorRoleNotes(
          selectedGroupId,
          actorId,
          nextRoleNotes,
          actors.map((item) => String(item.id || "").trim()).filter(Boolean)
        );
        if (!roleNotesResp.ok) {
          showError(roleNotesResp.error);
          return;
        }
        editActorRoleNotesBaselineRef.current = nextRoleNotes;
      }

      if (options.restart) {
        const restartResp = await api.restartActor(selectedGroupId, actorId);
        if (!restartResp.ok) {
          showError(`${restartResp.error.code}: ${restartResp.error.message}`);
          return;
        }
      }

      await refreshActors();
      setEditingActor(null);

      if (!options.restart) {
        const isRunning = Boolean(editingActor.running ?? editingActor.enabled ?? false);
        const restartRequired = isRunning && (willChangeSecrets || profileChanged || runtimeChanged || commandChanged || convertToCustom);
        if (restartRequired) {
          showNotice({ message: t("savedRestartRequired", { label }) });
        }
      }
    } finally {
      setBusy("");
    }
  };

  const handleSaveEditActorOnly = async (payload: EditActorSavePayload) => {
    await handleSaveEditActor(payload, { restart: false });
  };

  const handleSaveEditActorAndRestart = async (payload: EditActorSavePayload) => {
    await handleSaveEditActor(payload, { restart: true });
  };

  const applyEditingActor = useCallback((actor: Record<string, unknown>) => {
    const runtime = String(actor.runtime || "").trim();
    setEditActorRuntime((runtime || "codex") as SupportedRuntime);
    setEditActorRunner(getEffectiveActorRunner(actor));
    setEditActorCommand(Array.isArray(actor.command) ? actor.command.join(" ") : "");
    setEditActorTitle(String(actor.title || ""));
    setEditActorRoleNotes("");
    editActorRoleNotesBaselineRef.current = "";
    setEditActorCapabilityAutoloadText(
      formatCapabilityIdInput((actor as { capability_autoload?: unknown[] }).capability_autoload)
    );
    setEditingActor(actor as Actor);
  }, [setEditActorRuntime, setEditActorRunner, setEditActorCommand, setEditActorTitle, setEditActorRoleNotes, setEditActorCapabilityAutoloadText, setEditingActor]);

  useEffect(() => {
    if (!editingActor || !selectedGroupId) return;
    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;
    void loadEditingActorRoleNotes(selectedGroupId, actorId);
  }, [editingActor, selectedGroupId, loadEditingActorRoleNotes]);

  useEffect(() => {
    if (!editingActor) return;
    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;
    const latest = actors.find((item) => String(item.id || "").trim() === actorId);
    if (!latest) return;
    const configChanged =
      String(editingActor.profile_id || "").trim() !== String(latest.profile_id || "").trim() ||
      String(editingActor.profile_scope || "global").trim() !== String(latest.profile_scope || "global").trim() ||
      String(editingActor.profile_owner || "").trim() !== String(latest.profile_owner || "").trim() ||
      Number(editingActor.profile_revision_applied || 0) !== Number(latest.profile_revision_applied || 0) ||
      String(editingActor.runtime || "").trim() !== String(latest.runtime || "").trim() ||
      getEffectiveActorRunner(editingActor) !== getEffectiveActorRunner(latest) ||
      String(editingActor.title || "") !== String(latest.title || "") ||
      String(Array.isArray(editingActor.command) ? editingActor.command.join("\u0000") : "") !==
        String(Array.isArray(latest.command) ? latest.command.join("\u0000") : "") ||
      String(
        normalizeCapabilityIdList((editingActor as { capability_autoload?: unknown[] }).capability_autoload).join(
          "\u0000"
        )
      ) !==
        String(
          normalizeCapabilityIdList((latest as { capability_autoload?: unknown[] }).capability_autoload).join("\u0000")
        );
    if (configChanged) {
      applyEditingActor(latest as Record<string, unknown>);
      return;
    }

    const avatarChanged =
      String(editingActor.avatar_url || "") !== String(latest.avatar_url || "") ||
      Boolean(editingActor.has_custom_avatar) !== Boolean(latest.has_custom_avatar);

    if (avatarChanged) {
      setEditingActor(latest);
    }
  }, [actors, editingActor, applyEditingActor, setEditingActor]);

  const handleSaveEditActorAsProfile = async (): Promise<SaveActorProfileResult | void> => {
    if (!editingActor || !selectedGroupId) return;
    const suggested = String(editActorTitle || editingActor.title || editingActor.id || "New Profile").trim();
    const name = window.prompt(t("profileNamePrompt"), suggested);
    if (!name || !name.trim()) return;
    setBusy("actor-profile-save");
    try {
      const resp = await api.upsertActorProfile({
        name: name.trim(),
        runtime: editActorRuntime,
        runner: editActorRunner,
        command: editActorCommand.trim(),
        submit: String(editingActor.submit || "enter"),
        env: editingActor.env && typeof editingActor.env === "object" ? editingActor.env : {},
        capability_defaults: {
          autoload_capabilities: parseCapabilityIdInput(editActorCapabilityAutoloadText),
          default_scope: "actor",
          session_ttl_seconds: 3600,
        },
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const profileId = String(resp.result?.profile?.id || "").trim();
      if (profileId) {
        const copyResp = await api.copyActorPrivateEnvToProfile(profileId, selectedGroupId, editingActor.id);
        if (!copyResp.ok) {
          showError(`${copyResp.error.code}: ${copyResp.error.message}`);
          return;
        }
      }
      await loadActorProfiles();
      const profileName = String(resp.result?.profile?.name || "").trim() || name.trim();
      showNotice({ message: t("savedToActorProfiles") });
      if (!profileId) return;
      const useNow = window.confirm(
        t("useSavedProfileNowConfirm", {
          name: profileName,
          actor: String(editActorTitle || editingActor.title || editingActor.id || "").trim() || editingActor.id,
        })
      );
      return { profileId, profileName, useNow };
    } finally {
      setBusy("");
    }
  };

  const handleFetchDirContents = async (path: string) => {
    setShowDirBrowser(true);
    setDirBrowseError("");
    const resp = await api.fetchDirContents(path);
    if (resp.ok) {
      setDirItems(resp.result.items || []);
      setCurrentDir(resp.result.path || path);
      setParentDir(resp.result.parent || null);
    } else {
      setDirBrowseError(resp.error?.message || t('failedToListDir'));
    }
  };

  const handleSelectCreateGroupTemplate = async (file: File | null) => {
    setCreateGroupTemplateFile(file);
    setCreateTemplatePreview(null);
    setCreateTemplateError("");
    if (!file) return;

    setCreateTemplateBusy(true);
    try {
      const resp = await api.previewTemplate(file);
      if (!resp.ok) {
        setCreateTemplateError(resp.error?.message || t('invalidTemplate'));
        return;
      }
      setCreateTemplatePreview(resp.result?.template || null);
    } catch {
      setCreateTemplateError(t('failedToLoadTemplate'));
    } finally {
      setCreateTemplateBusy(false);
    }
  };

  const handleCreateGroup = async () => {
    const path = createGroupPath.trim();
    if (!path) return;
    const dirName = path.split("/").filter(Boolean).pop() || "working-group";
    const title = createGroupName.trim() || dirName;
    setBusy("create");
    try {
      let groupId = "";

      if (createGroupTemplateFile) {
        const resp = await api.createGroupFromTemplate(path, title, "", createGroupTemplateFile);
        if (!resp.ok) {
          if (resp.error?.code === "scope_already_attached") {
            const existing = getErrorDetailGroupId(resp.error);
            if (existing) {
              showError(t('scopeAlreadyAttached'));
              closeModal("createGroup");
              resetCreateGroupForm();
              setCreateTemplatePreview(null);
              setCreateTemplateError("");
              setCreateTemplateBusy(false);
              await refreshGroups();
              setSelectedGroupId(existing);
              return;
            }
          }
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        groupId = resp.result.group_id;
      } else {
        const resp = await api.createGroup(title);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        groupId = resp.result.group_id;
        const attachResp = await api.attachScope(groupId, path);
        if (!attachResp.ok) {
          showError(t('createdButFailedAttach', { message: attachResp.error.message }));
        }
      }

      resetCreateGroupForm();
      setCreateTemplatePreview(null);
      setCreateTemplateError("");
      setCreateTemplateBusy(false);
      closeModal("createGroup");
      await refreshGroups();
      setSelectedGroupId(groupId);
    } finally {
      setBusy("");
    }
  };

  const handleAddActor = async (avatarFile?: File | null): Promise<boolean> => {
    if (!selectedGroupId) return false;
    const actorId = newActorId.trim();
    const secretsText = String(newActorSecretsSetText || "");
    const roleNotes = String(newActorRoleNotes || "").trim();
    const selectedProfile = actorProfiles.find((item) => actorProfileIdentityKey(item) === String(newActorProfileId || "").trim()) || null;
    const capabilityAutoload = parseCapabilityIdInput(newActorCapabilityAutoloadText);

    if (newActorUseProfile && !selectedProfile) {
      setAddActorError(t("selectProfileFirst"));
      return false;
    }

    let secretsSetVars: Record<string, string> = {};
    if (!newActorUseProfile) {
      const parsedSecrets = parsePrivateEnvSetText(secretsText);
      if (!parsedSecrets.ok) {
        setAddActorError(parsedSecrets.error);
        return false;
      }
      secretsSetVars = parsedSecrets.setVars;
    }

    setBusy("actor-add");
    setAddActorError("");
    try {
      const commandToUse = newActorUseProfile ? "" : (newActorUseDefaultCommand ? "" : newActorCommand);
      const resp = await api.addActor(
        selectedGroupId,
        actorId,
        newActorRole,
        newActorUseProfile ? String(selectedProfile?.runtime || "codex") : newActorRuntime,
        newActorUseProfile
          ? normalizeActorRunner(selectedProfile?.runner)
          : newActorRunner,
        commandToUse,
        newActorUseProfile ? undefined : (Object.keys(secretsSetVars).length ? secretsSetVars : undefined),
        newActorUseProfile
          ? {
              profileId: String(selectedProfile?.id || "").trim(),
              profileScope: (selectedProfile?.scope || "global") as api.ProfileScope,
              profileOwner: String(selectedProfile?.owner_id || "").trim() || undefined,
              capabilityAutoload,
            }
          : {
              capabilityAutoload,
            }
      );
      if (!resp.ok) {
        setAddActorError(resp.error?.message || t('failedToAddAgent'));
        return false;
      }

      const createdActorId = String(
        (resp.result && typeof resp.result === "object"
          ? (resp.result as { actor?: { id?: string } }).actor?.id
          : "") || actorId || suggestedActorId
      ).trim();

      const postCreateErrors: string[] = [];

      if (roleNotes && createdActorId) {
        const roleNotesResp = await persistActorRoleNotes(
          selectedGroupId,
          createdActorId,
          roleNotes,
          [...actors.map((item) => String(item.id || "").trim()).filter(Boolean), createdActorId]
        );
        if (!roleNotesResp.ok) {
          postCreateErrors.push(`${t("roleNotes")}: ${roleNotesResp.error}`);
        }
      }

      if (avatarFile && createdActorId) {
        const avatarResp = await api.uploadActorAvatar(selectedGroupId, createdActorId, avatarFile);
        if (!avatarResp.ok) {
          postCreateErrors.push(`${t("avatarTitle")}: ${avatarResp.error?.message || t("avatarUploadFailed")}`);
        }
      }

      closeModal("addActor");
      resetAddActorForm();
      await refreshActors();
      if (postCreateErrors.length > 0) {
        showError(
          t("actorCreatedSetupFailed", {
            actor: createdActorId,
            details: postCreateErrors.join(" · "),
          })
        );
      }
      return true;
    } finally {
      setBusy("");
    }
  };

  const handleSaveNewActorAsProfile = async () => {
    if (newActorUseProfile) return;
    const suggested = String(newActorId || `${newActorRuntime}-profile`).trim();
    const name = window.prompt(t("profileNamePrompt"), suggested);
    if (!name || !name.trim()) return;
    setBusy("actor-profile-save");
    try {
      const commandToUse = newActorUseDefaultCommand ? "" : newActorCommand.trim();
      const resp = await api.upsertActorProfile({
        name: name.trim(),
        runtime: newActorRuntime,
        runner: newActorRunner,
        command: commandToUse,
        submit: "enter",
        env: {},
        capability_defaults: {
          autoload_capabilities: parseCapabilityIdInput(newActorCapabilityAutoloadText),
          default_scope: "actor",
          session_ttl_seconds: 3600,
        },
      });
      if (!resp.ok) {
        setAddActorError(resp.error?.message || t("failedToSaveActorProfile"));
        return;
      }
      const profileId = String(resp.result?.profile?.id || "").trim();
      if (profileId) {
        const parsed = parsePrivateEnvSetText(newActorSecretsSetText);
        if (!parsed.ok) {
          setAddActorError(parsed.error);
          return;
        }
        const hasSecrets = Object.keys(parsed.setVars).length > 0;
        if (hasSecrets) {
          const secretResp = await api.updateActorProfilePrivateEnv(profileId, parsed.setVars, [], false);
          if (!secretResp.ok) {
            setAddActorError(secretResp.error?.message || t("failedToSaveActorProfile"));
            return;
          }
        }
      }
      showNotice({ message: t("savedToActorProfiles") });
      await loadActorProfiles();
    } finally {
      setBusy("");
    }
  };

  // Computed for AddActorModal
  const suggestedActorId = (() => {
    const selectedProfile = actorProfiles.find((item) => actorProfileIdentityKey(item) === String(newActorProfileId || "").trim()) || null;
    const profileRuntime = String(selectedProfile?.runtime || "").trim();
    const prefix = newActorUseProfile ? (profileRuntime || "actor") : (newActorRuntime === "web_model" ? "chatgpt-web" : newActorRuntime);
    const existing = new Set(actors.map((a) => String(a.id || "")));
    for (let i = 1; i <= 999; i++) {
      const candidate = `${prefix}-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return `${prefix}-${Date.now()}`;
  })();
  const currentGroupHasChatGptWebModelActor = actors.some(
    (actor) => String(actor.runtime || "").trim().toLowerCase() === "web_model"
  );

  const canAddActor = (() => {
    if (busy === "actor-add") return false;
    if (newActorUseProfile) return Boolean(String(newActorProfileId || "").trim());
    if (newActorRuntime === "web_model" && currentGroupHasChatGptWebModelActor) return false;
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (!newActorUseDefaultCommand && !newActorCommand.trim()) return false;
    if (newActorRuntime === "custom" && (newActorUseDefaultCommand || !newActorCommand.trim())) return false;
    if (!available && (newActorUseDefaultCommand || !newActorCommand.trim())) return false;
    return true;
  })();

  const addActorDisabledReason = (() => {
    if (busy === "actor-add") return "";
    if (newActorUseProfile && !String(newActorProfileId || "").trim()) {
      return t("profileRequired");
    }
    if (!newActorUseProfile && newActorRuntime === "web_model" && currentGroupHasChatGptWebModelActor) {
      return "This group already has the ChatGPT Web Model actor. Use Settings > ChatGPT Web Model to configure it.";
    }
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (!newActorUseDefaultCommand && !newActorCommand.trim()) {
      return t("commandOverrideRequired");
    }
    if (newActorRuntime === "custom" && (newActorUseDefaultCommand || !newActorCommand.trim())) {
      return t('customRuntimeRequiresCommand');
    }
    if (!available && (newActorUseDefaultCommand || !newActorCommand.trim())) {
      return t('runtimeNotInstalled', { runtime: RUNTIME_INFO[newActorRuntime]?.label || newActorRuntime });
    }
    return "";
  })();

  const handleCloseAddActor = useCallback(
    () => closeModal("addActor"),
    [closeModal]
  );

  const handleCancelEditActor = useCallback(() => {
    editActorRoleNotesSeqRef.current += 1;
    editActorRoleNotesBaselineRef.current = "";
    setEditActorRoleNotesBusy(false);
    setEditActorRoleNotes("");
    setEditingActor(null);
  }, [setEditActorRoleNotes, setEditingActor]);

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
    return (events || []).find((ev) => String(ev.id || "") === eid) || null;
  }, [chatWindow, events, relayEventId, relaySource, selectedGroupId]);

  const presentationReferenceEvents = useMemo(() => {
    const next = new Map<string, LedgerEvent>();
    for (const event of events || []) {
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
  }, [chatWindow, events, selectedGroupId]);

  const presentationViewerSourceEvent = useMemo(() => {
    const focusEventId = String(presentationViewer?.focusEventId || "").trim();
    if (!focusEventId || presentationViewer?.groupId !== selectedGroupId) return null;
    return presentationReferenceEvents.find((event) => String(event.id || "").trim() === focusEventId) || null;
  }, [presentationReferenceEvents, presentationViewer, selectedGroupId]);

  const handleRelayMessage = async (dstGroupId: string, toTokens: string[], note: string) => {
    const src = relaySourceEvent;
     const srcGroupId = relaySourceGroupId;
    const dstGroup = String(dstGroupId || "").trim();
    const srcEventId = src?.id ? String(src.id) : "";
    if (!src || !srcGroupId || !srcEventId) return;
    if (!dstGroup) return;
    if (dstGroup === srcGroupId) {
      showError(t('destGroupDifferent'));
      return;
    }

    const d = src.data as ChatMessageData | undefined;
    const srcText = typeof d?.text === "string" ? d.text : "";
    const srcQuoteText = typeof d?.quote_text === "string" ? d.quote_text.trim() : "";
    const noteText = String(note || "").trim();
    const relayText = (noteText ? noteText + "\n\n" : "") + String(srcText || "");
    if (!relayText.trim()) {
      showError(t('relayTextEmpty'));
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
        setPresentationViewer({ groupId: gid, slotId: resp.result.slot_id || payload.slotId, surface: preferredPresentationSurface });
      } finally {
        setBusy("");
      }
    },
    [preferredPresentationSurface, selectedGroupId, setBusy, setChatPresentationDockOpen, setGroupPresentation, setPresentationPin, setPresentationViewer, showError],
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
        setPresentationViewer({ groupId: gid, slotId: resp.result.slot_id || payload.slotId, surface: preferredPresentationSurface });
      } finally {
        setBusy("");
      }
    },
    [preferredPresentationSurface, selectedGroupId, setBusy, setChatPresentationDockOpen, setGroupPresentation, setPresentationPin, setPresentationViewer, showError],
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
        setPresentationViewer({ groupId: gid, slotId: resp.result.slot_id || payload.slotId, surface: preferredPresentationSurface });
      } finally {
        setBusy("");
      }
    },
    [preferredPresentationSurface, selectedGroupId, setBusy, setChatPresentationDockOpen, setGroupPresentation, setPresentationPin, setPresentationViewer, showError],
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
    [clearPresentationSlotAttention, forgetPresentationViewerSlot, selectedGroupId, setBusy, setGroupPresentation, setPresentationViewer, setPresentationPin, showError, t],
  );

  const handleQuotePresentationReference = useCallback(
    (payload: { slotId: string; ref?: PresentationMessageRef | null }) => {
      const gid = String(selectedGroupId || "").trim();
      const normalizedSlotId = String(payload.slotId || "").trim();
      if (!gid || !normalizedSlotId) return;
      const slot = findPresentationSlot(groupPresentation, normalizedSlotId);
      const ref = payload.ref || buildPresentationRefForSlot(slot);
      if (!ref) {
        showError(t("chat:presentationMissingCard", { defaultValue: "This presentation slot is empty." }));
        return;
      }
      setQuotedPresentationRef(ref);
      setComposerDestGroupId(gid);
      setActiveTab("chat");
      setChatMobileSurface(gid, "messages");
      setPresentationViewer(null);
      window.setTimeout(() => composerRef.current?.focus(), 0);
    },
    [composerRef, groupPresentation, selectedGroupId, setActiveTab, setChatMobileSurface, setComposerDestGroupId, setPresentationViewer, setQuotedPresentationRef, showError, t],
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
    [composerRef, onStartReply, openChatWindow, selectedGroupId, setActiveTab, setChatMobileSurface, setPresentationViewer],
  );

  return (
    <>
      <MobileMenuSheet
        isOpen={modals.mobileMenu}
        isDark={isDark}
        theme={theme}
        textScale={textScale}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        selectedGroupRunning={selectedGroupRunning}
        actors={actors}
        busy={busy}
        onClose={() => closeModal("mobileMenu")}
        onThemeChange={onThemeChange}
        onTextScaleChange={onTextScaleChange}
        onOpenSearch={() => openModal("search")}
        onOpenContext={() => {
          openModal("context");
        }}
        onOpenSettings={() => openModal("settings")}
        onOpenGroupEdit={canManageGroups ? () => {
          if (groupDoc) {
            setEditGroupTitle(groupDoc.title || "");
            setEditGroupTopic(groupDoc.topic || "");
            openModal("groupEdit");
          }
        } : undefined}
        onStartGroup={onStartGroup}
        onStopGroup={onStopGroup}
        onSetGroupState={onSetGroupState}
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
                findPresentationSlot(groupPresentation, presentationPin?.slotId || "")?.card?.published_at || "empty"
              }`
            : "presentation-pin-closed"
        }
        isOpen={!!presentationPin && presentationPin.groupId === selectedGroupId}
        groupId={selectedGroupId}
        isDark={isDark}
        slot={presentationPin?.groupId === selectedGroupId ? findPresentationSlot(groupPresentation, presentationPin?.slotId || "") : null}
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
                isOpen={!!presentationViewer && presentationViewer.surface !== "split" && presentationViewer.groupId === selectedGroupId && presentationViewer.slotId === slotId}
                isDark={isDark}
                readOnly={readOnly}
                groupId={selectedGroupId}
                slotId={slotId}
                presentation={groupPresentation}
                focusRef={presentationViewer?.groupId === selectedGroupId && presentationViewer.slotId === slotId ? presentationViewer.focusRef || null : null}
                focusEventId={presentationViewer?.groupId === selectedGroupId && presentationViewer.slotId === slotId ? presentationViewer.focusEventId || null : null}
                sourceEvent={presentationViewer?.groupId === selectedGroupId && presentationViewer.slotId === slotId ? presentationViewerSourceEvent : null}
                onQuoteInChat={handleQuotePresentationReference}
                onOpenMessageContext={(eventId) => void handleOpenPresentationMessageContext(eventId)}
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
        isSmallScreen={isSmallScreen}
        toLabel={messageMeta?.toLabel || ""}
        statusKind={messageMeta?.statusKind || "read"}
        entries={(messageMeta?.entries || []) as [string, boolean][]}
        onClose={() => setRecipientsModal(null)}
      />

      <InboxModal
        isOpen={modals.inbox}
        actorId={inboxActorId}
        actors={actors}
        messages={inboxMessages}
        busy={busy}
        onClose={() => closeModal("inbox")}
        onMarkAllRead={handleMarkAllRead}
      />

      <GroupEditModal
        isOpen={modals.groupEdit}
        busy={busy}
        groupId={selectedGroupId || groupDoc?.group_id || ""}
        ccccHome={ccccHome}
        projectRoot={
          (() => {
            const key = String(groupDoc?.active_scope_key || "").trim();
            const scopes = Array.isArray(groupDoc?.scopes) ? groupDoc?.scopes : [];
            const active = scopes.find((s) => String(s?.scope_key || "").trim() === key);
            const url = String(active?.url || scopes[0]?.url || "").trim();
            return url;
          })()
        }
        title={editGroupTitle}
        topic={editGroupTopic}
        onChangeTitle={setEditGroupTitle}
        onChangeTopic={setEditGroupTopic}
        onSave={handleSaveGroupEdit}
        onCancel={() => closeModal("groupEdit")}
        onDelete={handleDeleteGroup}
      />

      <EditActorModal
        isOpen={!!editingActor}
        isDark={isDark}
        busy={busy}
        groupId={selectedGroupId || groupDoc?.group_id || ""}
        actorId={editingActor?.id || ""}
        avatarUrl={editingActor?.avatar_url || undefined}
        hasCustomAvatar={!!editingActor?.has_custom_avatar}
        isRunning={!!(editingActor && (editingActor.running ?? editingActor.enabled ?? false))}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        runner={editActorRunner}
        onChangeRunner={setEditActorRunner}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        title={editActorTitle}
        onChangeTitle={setEditActorTitle}
        roleNotes={editActorRoleNotes}
        onChangeRoleNotes={setEditActorRoleNotes}
        roleNotesBusy={editActorRoleNotesBusy}
        capabilityAutoloadText={editActorCapabilityAutoloadText}
        onChangeCapabilityAutoloadText={setEditActorCapabilityAutoloadText}
        onSave={handleSaveEditActorOnly}
        onSaveAndRestart={handleSaveEditActorAndRestart}
        linkedProfileId={String(editingActor?.profile_id || "") || undefined}
        linkedProfileScope={(String(editingActor?.profile_scope || "global").trim() || "global") as "global" | "user"}
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
        createGroupTemplateFile={createGroupTemplateFile}
        templatePreview={createTemplatePreview}
        templateError={createTemplateError}
        templateBusy={createTemplateBusy}
        onSelectTemplate={handleSelectCreateGroupTemplate}
        dirBrowseError={dirBrowseError}
        onFetchDirContents={handleFetchDirContents}
        onCreateGroup={handleCreateGroup}
        onClose={() => closeModal("createGroup")}
        onCancelAndReset={() => {
          closeModal("createGroup");
          resetCreateGroupForm();
          setCreateTemplatePreview(null);
          setCreateTemplateError("");
          setCreateTemplateBusy(false);
          setDirBrowseError("");
        }}
      />

      <AddActorModal
        isOpen={modals.addActor}
        isDark={isDark}
        busy={busy}
        hasForeman={hasForeman}
        runtimes={runtimes}
        suggestedActorId={suggestedActorId}
        newActorId={newActorId}
        setNewActorId={setNewActorId}
        newActorRole={newActorRole}
        setNewActorRole={setNewActorRole}
        newActorUseProfile={newActorUseProfile}
        setNewActorUseProfile={setNewActorUseProfile}
        newActorProfileId={newActorProfileId}
        setNewActorProfileId={setNewActorProfileId}
        actorProfiles={actorProfiles}
        actorProfilesBusy={actorProfilesBusy}
        newActorRuntime={newActorRuntime}
        setNewActorRuntime={setNewActorRuntime}
        newActorRunner={newActorRunner}
        setNewActorRunner={setNewActorRunner}
        newActorCommand={newActorCommand}
        setNewActorCommand={setNewActorCommand}
        newActorUseDefaultCommand={newActorUseDefaultCommand}
        setNewActorUseDefaultCommand={setNewActorUseDefaultCommand}
        newActorSecretsSetText={newActorSecretsSetText}
        setNewActorSecretsSetText={setNewActorSecretsSetText}
        newActorCapabilityAutoloadText={newActorCapabilityAutoloadText}
        setNewActorCapabilityAutoloadText={setNewActorCapabilityAutoloadText}
        newActorRoleNotes={newActorRoleNotes}
        setNewActorRoleNotes={setNewActorRoleNotes}
        showAdvancedActor={showAdvancedActor}
        setShowAdvancedActor={setShowAdvancedActor}
        addActorError={addActorError}
        setAddActorError={setAddActorError}
        canAddActor={canAddActor}
        addActorDisabledReason={addActorDisabledReason}
        onAddActor={handleAddActor}
        onSaveAsProfile={handleSaveNewActorAsProfile}
        onClose={handleCloseAddActor}
        onCancelAndReset={() => {
          closeModal("addActor");
          resetAddActorForm();
        }}
      />
    </>
  );
}
