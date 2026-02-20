// AppModals renders all modal components in one place.
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextModal } from "./ContextModal";
import { SettingsModal } from "./SettingsModal";
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
import { RelayMessageModal } from "./modals/RelayMessageModal";
import { RecipientsModal } from "./modals/RecipientsModal";
import { parsePrivateEnvSetText } from "../utils/privateEnvInput";
import {
  useGroupStore,
  useUIStore,
  useModalStore,
  useInboxStore,
  useFormStore,
} from "../stores";
import { getAckRecipientIdsForEvent, getRecipientActorIdsForEvent } from "../hooks/useSSE";
import * as api from "../services/api";
import { ActorProfile, RUNTIME_INFO, LedgerEvent, GroupSettings, ChatMessageData, SupportedRuntime } from "../types";

interface AppModalsProps {
  isDark: boolean;
  ccccHome: string;
  composerRef: React.RefObject<HTMLTextAreaElement>;
  onStartReply: (ev: LedgerEvent) => void;
  onThemeToggle: () => void;
  onStartGroup: () => Promise<void>;
  onStopGroup: () => Promise<void>;
  onSetGroupState: (state: "active" | "idle" | "paused") => Promise<void>;
  fetchContext: (groupId: string) => Promise<void>;
}

function getErrorDetailGroupId(err: unknown): string {
  if (!err || typeof err !== "object") return "";
  const details = (err as { details?: unknown }).details;
  if (!details || typeof details !== "object") return "";
  return String((details as { group_id?: unknown }).group_id || "").trim();
}

export function AppModals({
  isDark,
  ccccHome,
  composerRef,
  onStartReply,
  onThemeToggle,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  fetchContext,
}: AppModalsProps) {
  const { t } = useTranslation('actors');
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
    runtimes,
    setSelectedGroupId,
    setGroupDoc,
    setGroupContext,
    setGroupSettings,
    refreshGroups,
    refreshActors,
    loadGroup,
    openChatWindow,
  } = useGroupStore();

  const {
    busy,
    isSmallScreen,
    setBusy,
    showError,
    showNotice,
    setActiveTab,
  } = useUIStore();

  const {
    modals,
    recipientsEventId: _recipientsEventId,
    relayEventId,
    relaySource,
    editingActor,
    openModal,
    closeModal,
    setRecipientsModal,
    setRelayModal,
    setEditingActor,
  } = useModalStore();

  const { inboxActorId, inboxMessages, setInboxMessages } = useInboxStore();

  const {
    editGroupTitle,
    editGroupTopic,
    setEditGroupTitle,
    setEditGroupTopic,
    editActorRuntime,
    editActorCommand,
    editActorTitle,
    setEditActorRuntime,
    setEditActorCommand,
    setEditActorTitle,
    newActorId,
    newActorRole,
    newActorRuntime,
    newActorCommand,
    newActorUseDefaultCommand,
    newActorSecretsSetText,
    newActorUseProfile,
    newActorProfileId,
    showAdvancedActor,
    addActorError,
    setNewActorId,
    setNewActorRole,
    setNewActorRuntime,
    setNewActorCommand,
    setNewActorUseDefaultCommand,
    setNewActorSecretsSetText,
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
  const [actorProfiles, setActorProfiles] = useState<ActorProfile[]>([]);
  const [actorProfilesBusy, setActorProfilesBusy] = useState(false);

  // Computed
  const selectedGroupRunning = useGroupStore(
    (s) => s.groups.find((g) => String(g.group_id || "") === s.selectedGroupId)?.running ?? false
  );
  const hasForeman = actors.some((a) => a.role === "foreman");

  // Compute messageMeta for RecipientsModal (moved from App.tsx)
  const messageMetaEvent = useMemo(() => {
    if (!_recipientsEventId) return null;
    return (
      events.find(
        (x) => x.kind === "chat.message" && String(x.id || "") === _recipientsEventId
      ) || null
    );
  }, [events, _recipientsEventId]);

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

  const loadActorProfiles = async () => {
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
  };

  useEffect(() => {
    if (!modals.addActor && !editingActor) return;
    void loadActorProfiles();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modals.addActor, editingActor]);

  // Handlers
  const handleUpdateVision = async (vision: string) => {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await api.updateVision(selectedGroupId, vision);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      void fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleUpdateSketch = async (sketch: string) => {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await api.updateSketch(selectedGroupId, sketch);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      void fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  };

  const handleUpdateSettings = async (settings: Partial<GroupSettings>) => {
    if (!selectedGroupId) return;
    setBusy("settings-update");
    try {
      const resp = await api.updateSettings(selectedGroupId, settings);
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      const settingsResp = await api.fetchSettings(selectedGroupId);
      if (settingsResp.ok && settingsResp.result.settings) {
        setGroupSettings(settingsResp.result.settings);
      }
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
    const profileId = String(payload.profileId || "").trim();
    const linkedBefore = Boolean(String(editingActor.profile_id || "").trim());
    const convertToCustom = mode === "custom" && linkedBefore && !!payload.convertToCustom;

    if (mode === "profile" && !profileId) {
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
    const currentCommand = Array.isArray(editingActor.command)
      ? editingActor.command.filter((item) => typeof item === "string" && item.trim()).join(" ").trim()
      : "";
    const currentTitle = String(editingActor.title || "").trim();
    const nextRuntime = String(editActorRuntime || "codex").trim();
    const nextCommand = String(editActorCommand || "").trim();
    const nextTitle = String(editActorTitle || "").trim();

    const runtimeChanged = mode === "custom" && (!linkedBefore || convertToCustom) && nextRuntime !== currentRuntime;
    const commandChanged = mode === "custom" && (!linkedBefore || convertToCustom) && nextCommand !== currentCommand;
    const titleChanged = nextTitle !== currentTitle;
    const profileChanged = mode === "profile" && profileId !== String(editingActor.profile_id || "").trim();
    const hasActorMutation = convertToCustom || runtimeChanged || commandChanged || titleChanged || profileChanged;

    if (!options.restart && !hasActorMutation && !willChangeSecrets) {
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
          nextTitle,
          { profileAction: "convert_to_custom" }
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
        const needProfilePatch = profileChanged || titleChanged;
        if (needProfilePatch) {
          const profileResp = await api.updateActor(
            selectedGroupId,
            actorId,
            undefined,
            undefined,
            nextTitle,
            { profileId }
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
        const snapshotTitle = String(actorSnapshot.title || "").trim();
        const needCustomPatch =
          nextRuntime !== snapshotRuntime || nextCommand !== snapshotCommand || nextTitle !== snapshotTitle;
        if (needCustomPatch) {
          const customResp = await api.updateActor(
            selectedGroupId,
            actorId,
            editActorRuntime,
            editActorCommand,
            nextTitle
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

  const _applyEditingActor = (actor: Record<string, unknown>) => {
    const runtime = String(actor.runtime || "").trim();
    setEditActorRuntime((runtime || "codex") as SupportedRuntime);
    setEditActorCommand(Array.isArray(actor.command) ? actor.command.join(" ") : "");
    setEditActorTitle(String(actor.title || ""));
    setEditingActor(actor as any);
  };

  useEffect(() => {
    if (!editingActor) return;
    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;
    const latest = actors.find((item) => String(item.id || "").trim() === actorId);
    if (!latest) return;
    const changed =
      String(editingActor.profile_id || "").trim() !== String(latest.profile_id || "").trim() ||
      Number(editingActor.profile_revision_applied || 0) !== Number(latest.profile_revision_applied || 0) ||
      String(editingActor.runtime || "").trim() !== String(latest.runtime || "").trim() ||
      String(editingActor.title || "") !== String(latest.title || "") ||
      String(Array.isArray(editingActor.command) ? editingActor.command.join("\u0000") : "") !==
        String(Array.isArray(latest.command) ? latest.command.join("\u0000") : "");
    if (changed) _applyEditingActor(latest as Record<string, unknown>);
  }, [actors, editingActor]);

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
        runner: String(editingActor.runner || "pty"),
        command: editActorCommand.trim(),
        submit: String(editingActor.submit || "enter"),
        env: editingActor.env && typeof editingActor.env === "object" ? editingActor.env : {},
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
    const resp = await api.fetchDirContents(path);
    if (resp.ok) {
      setDirItems(resp.result.items || []);
      setCurrentDir(resp.result.path || path);
      setParentDir(resp.result.parent || null);
    } else {
      showError(resp.error?.message || t('failedToListDir'));
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

  const handleAddActor = async () => {
    if (!selectedGroupId) return;
    const actorId = newActorId.trim();
    const secretsText = String(newActorSecretsSetText || "");
    const selectedProfile = actorProfiles.find((item) => String(item.id || "") === String(newActorProfileId || "")) || null;

    if (newActorUseProfile && !selectedProfile) {
      setAddActorError(t("selectProfileFirst"));
      return;
    }

    let secretsSetVars: Record<string, string> = {};
    if (!newActorUseProfile) {
      const parsedSecrets = parsePrivateEnvSetText(secretsText);
      if (!parsedSecrets.ok) {
        setAddActorError(parsedSecrets.error);
        return;
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
        commandToUse,
        newActorUseProfile ? undefined : (Object.keys(secretsSetVars).length ? secretsSetVars : undefined),
        newActorUseProfile
          ? {
              profileId: String(selectedProfile?.id || "").trim(),
              runner: String(selectedProfile?.runner || "pty") === "headless" ? "headless" : "pty",
            }
          : undefined
      );
      if (!resp.ok) {
        setAddActorError(resp.error?.message || t('failedToAddAgent'));
        return;
      }

      closeModal("addActor");
      resetAddActorForm();
      await refreshActors();
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
        runner: "pty",
        command: commandToUse,
        submit: "enter",
        env: {},
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
    const selectedProfile = actorProfiles.find((item) => String(item.id || "") === String(newActorProfileId || "")) || null;
    const profileRuntime = String(selectedProfile?.runtime || "").trim();
    const prefix = newActorUseProfile ? (profileRuntime || "actor") : newActorRuntime;
    const existing = new Set(actors.map((a) => String(a.id || "")));
    for (let i = 1; i <= 999; i++) {
      const candidate = `${prefix}-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return `${prefix}-${Date.now()}`;
  })();

  const canAddActor = (() => {
    if (busy === "actor-add") return false;
    if (newActorUseProfile) return Boolean(String(newActorProfileId || "").trim());
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
    setEditingActor(null);
  }, [setEditingActor]);

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
    const noteText = String(note || "").trim();
    const relayText = (noteText ? noteText + "\n\n" : "") + String(srcText || "");
    if (!relayText.trim()) {
      showError(t('relayTextEmpty'));
      return;
    }

    const to = (toTokens || []).map((t) => String(t || "").trim()).filter((t) => t);

    setBusy("relay");
    try {
      const resp = await api.relayMessage(dstGroup, relayText, to, { groupId: srcGroupId, eventId: srcEventId });
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

  return (
    <>
      <MobileMenuSheet
        isOpen={modals.mobileMenu}
        isDark={isDark}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        selectedGroupRunning={selectedGroupRunning}
        actors={actors}
        busy={busy}
        onClose={() => closeModal("mobileMenu")}
        onToggleTheme={onThemeToggle}
        onOpenSearch={() => openModal("search")}
        onOpenContext={() => {
          if (selectedGroupId) void fetchContext(selectedGroupId);
          openModal("context");
        }}
        onOpenSettings={() => openModal("settings")}
        onOpenGroupEdit={() => {
          if (groupDoc) {
            setEditGroupTitle(groupDoc.title || "");
            setEditGroupTopic(groupDoc.topic || "");
            openModal("groupEdit");
          }
        }}
        onStartGroup={onStartGroup}
        onStopGroup={onStopGroup}
        onSetGroupState={onSetGroupState}
      />

      {modals.relay && relayEventId ? (
        <RelayMessageModal
          key={`${selectedGroupId}:${relayEventId}`}
          isOpen={true}
          isDark={isDark}
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

      <ContextModal
        isOpen={modals.context}
        onClose={() => closeModal("context")}
        groupId={selectedGroupId}
        context={groupContext}
        onRefreshContext={async () => {
          if (selectedGroupId) await fetchContext(selectedGroupId);
        }}
        onUpdateVision={handleUpdateVision}
        onUpdateSketch={handleUpdateSketch}
        busy={busy === "context-update"}
        isDark={isDark}
      />

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

      <RecipientsModal
        isOpen={!!messageMeta}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        toLabel={messageMeta?.toLabel || ""}
        statusKind={messageMeta?.statusKind || "read"}
        entries={(messageMeta?.entries || []) as [string, boolean][]}
        onClose={() => setRecipientsModal(null)}
      />

      <InboxModal
        isOpen={modals.inbox}
        isDark={isDark}
        actorId={inboxActorId}
        actors={actors}
        messages={inboxMessages}
        busy={busy}
        onClose={() => closeModal("inbox")}
        onMarkAllRead={handleMarkAllRead}
      />

      <GroupEditModal
        isOpen={modals.groupEdit}
        isDark={isDark}
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
        isRunning={!!(editingActor && (editingActor.running ?? editingActor.enabled ?? false))}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        title={editActorTitle}
        onChangeTitle={setEditActorTitle}
        onSave={handleSaveEditActorOnly}
        onSaveAndRestart={handleSaveEditActorAndRestart}
        linkedProfileId={String(editingActor?.profile_id || "") || undefined}
        actorProfiles={actorProfiles}
        actorProfilesBusy={actorProfilesBusy}
        onSaveAsProfile={handleSaveEditActorAsProfile}
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
        createGroupTemplateFile={createGroupTemplateFile}
        templatePreview={createTemplatePreview}
        templateError={createTemplateError}
        templateBusy={createTemplateBusy}
        onSelectTemplate={handleSelectCreateGroupTemplate}
        onFetchDirContents={handleFetchDirContents}
        onCreateGroup={handleCreateGroup}
        onClose={() => closeModal("createGroup")}
        onCancelAndReset={() => {
          closeModal("createGroup");
          resetCreateGroupForm();
          setCreateTemplatePreview(null);
          setCreateTemplateError("");
          setCreateTemplateBusy(false);
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
        newActorCommand={newActorCommand}
        setNewActorCommand={setNewActorCommand}
        newActorUseDefaultCommand={newActorUseDefaultCommand}
        setNewActorUseDefaultCommand={setNewActorUseDefaultCommand}
        newActorSecretsSetText={newActorSecretsSetText}
        setNewActorSecretsSetText={setNewActorSecretsSetText}
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
