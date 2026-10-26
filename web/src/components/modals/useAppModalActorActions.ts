import { useCallback, useEffect, useRef, useState } from "react";
import type { TFunction } from "i18next";
import {
  type Actor,
  type ActorProfile,
  isWebModelRuntime,
  RUNTIME_INFO,
  type RuntimeInfo,
  type SupportedRuntime,
  supportsAcpMode,
} from "../../types";
import * as api from "../../services/api";
import { useGroupStore, useUIStore } from "../../stores";
import { actorProfileIdentityKey, actorProfileMatchesRef } from "../../utils/actorProfiles";
import { normalizeCapabilityIdList, parseCapabilityIdInput } from "../../utils/capabilityAutoload";
import { parseHelpMarkdown, updateActorHelpNote } from "../../utils/helpMarkdown";
import { parsePrivateEnvSetText } from "../../utils/privateEnvInput";
import { runtimeDetectionKey } from "../../utils/runtimeDetection";
import { isGrokBotUrl } from "../../utils/webModelTargetDraft";
import type { CreatedNativeActorNavigation } from "../../hooks/useAppTabState";
import type { EditActorSavePayload, SaveActorProfileResult } from "./ActorConfigModal";
import { resolveNewActorId } from "./actorCreateModel";
import type { ActorSecretSaveChanges } from "./actorSecretManagerModel";
import { formatRuntimeCommand } from "./runtimeProfileControlsModel";

export function useAppModalActorActions(args: {
  selectedGroupId: string;
  actors: Actor[];
  runtimes: RuntimeInfo[];
  runtimeDetectionStatus: "idle" | "loading" | "ready" | "error";
  busy: string;
  setBusy: (busy: string) => void;
  showError: (msg: string) => void;
  showNotice: (notice: { message: string }) => void;
  addActorModalOpen: boolean;
  editingActor: Actor | null;
  setEditingActor: (actor: Actor | null) => void;
  closeModal: (modal: "addActor") => void;
  refreshActors: (groupId?: string, opts?: { includeUnread?: boolean }) => Promise<void>;
  onNativeActorCreated?: (visit: CreatedNativeActorNavigation) => void;
  editActorRuntime: SupportedRuntime;
  editActorCommand: string;
  editActorTitle: string;
  editActorNotes: string;
  editActorCapabilityAutoloadText: string;
  setEditActorNotes: (v: string) => void;
  newActorId: string;
  newActorRole: "peer" | "foreman";
  newActorRuntime: SupportedRuntime;
  newActorCommand: string;
  newActorUseDefaultCommand: boolean;
  newActorSecretsSetText: string;
  newActorCapabilityAutoloadText: string;
  newActorNotes: string;
  newActorUseProfile: boolean;
  newActorProfileId: string;
  setAddActorError: (v: string) => void;
  resetAddActorForm: () => void;
  t: TFunction<["actors", "chat", "modals"]>;
}) {
  const {
    selectedGroupId,
    actors,
    runtimes,
    runtimeDetectionStatus,
    busy,
    setBusy,
    showError,
    showNotice,
    addActorModalOpen,
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
  } = args;

  const [actorProfiles, setActorProfiles] = useState<ActorProfile[]>([]);
  const [actorProfilesBusy, setActorProfilesBusy] = useState(false);
  const [editActorNotesBusy, setEditActorNotesBusy] = useState(false);
  const editProfileSaveRef = useRef<{ profile: ActorProfile; copied: boolean } | null>(null);
  const newProfileSaveRef = useRef<ActorProfile | null>(null);
  const editActorNotesBaselineRef = useRef("");
  const editActorNotesSeqRef = useRef(0);
  const editActorVisitRef = useRef(0);
  const profileLoadSeqRef = useRef(0);

  useEffect(
    () => () => {
      editActorVisitRef.current += 1;
      editActorNotesSeqRef.current += 1;
      profileLoadSeqRef.current += 1;
    },
    [],
  );

  useEffect(() => {
    if (!addActorModalOpen) newProfileSaveRef.current = null;
  }, [addActorModalOpen]);

  const editingActorId = editingActor?.id;
  useEffect(() => {
    editProfileSaveRef.current = null;
    editActorVisitRef.current += 1;
    editActorNotesSeqRef.current += 1;
    editActorNotesBaselineRef.current = "";
    setEditActorNotesBusy(false);
  }, [selectedGroupId, editingActorId]);

  const loadEditingActorNotes = useCallback(
    async (groupId: string, actorId: string) => {
      const gid = String(groupId || "").trim();
      const aid = String(actorId || "").trim();
      if (!gid || !aid) {
        editActorNotesBaselineRef.current = "";
        setEditActorNotes("");
        return;
      }
      const seq = ++editActorNotesSeqRef.current;
      setEditActorNotesBusy(true);
      try {
        const resp = await api.fetchGroupPrompts(gid);
        if (!resp.ok) {
          if (seq === editActorNotesSeqRef.current) {
            editActorNotesBaselineRef.current = "";
            setEditActorNotes("");
          }
          return;
        }
        const helpContent = String(resp.result?.help?.content || "");
        const parsed = parseHelpMarkdown(helpContent);
        const note = String(parsed.actorNotes[aid] || "");
        if (seq !== editActorNotesSeqRef.current) return;
        editActorNotesBaselineRef.current = note.trim();
        setEditActorNotes(note);
      } finally {
        if (seq === editActorNotesSeqRef.current) {
          setEditActorNotesBusy(false);
        }
      }
    },
    [setEditActorNotes],
  );

  const persistActorNotes = useCallback(
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
    [],
  );

  const loadActorProfiles = useCallback(async () => {
    const seq = ++profileLoadSeqRef.current;
    setActorProfilesBusy(true);
    try {
      const resp = await api.listActorProfiles();
      if (seq !== profileLoadSeqRef.current) return;
      if (!resp.ok) {
        showError(resp.error?.message || t("failedToLoadActorProfiles"));
        return;
      }
      setActorProfiles(Array.isArray(resp.result?.profiles) ? resp.result.profiles : []);
    } finally {
      if (seq === profileLoadSeqRef.current) setActorProfilesBusy(false);
    }
  }, [showError, t]);

  useEffect(() => {
    if (addActorModalOpen) {
      void loadActorProfiles();
      return;
    }
    const linkedProfileId = String(editingActor?.profile_id || "").trim();
    if (!linkedProfileId) return;
    void loadActorProfiles();
  }, [addActorModalOpen, editingActor?.profile_id, loadActorProfiles]);

  const handleSaveEditActor = async (
    payload: EditActorSavePayload,
    options: { restart: boolean },
  ) => {
    if (!selectedGroupId || !editingActor) return;

    const savedActor = editingActor;
    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;

    const label = String(savedActor.title || editingActor.id || actorId).trim() || actorId;
    const mode = payload.mode === "profile" ? "profile" : "custom";
    const profileSelectionKey = String(payload.profileId || "").trim();
    const selectedProfile =
      mode === "profile"
        ? actorProfiles.find((item) => actorProfileIdentityKey(item) === profileSelectionKey) ||
          null
        : null;
    const profileId = String(selectedProfile?.id || "").trim();
    const linkedBefore = Boolean(String(savedActor.profile_id || "").trim());
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
    const canEditSecrets =
      mode === "custom" &&
      !isWebModelRuntime(editActorRuntime) &&
      (!linkedBefore || convertToCustom);
    const willChangeSecrets =
      canEditSecrets && (clear || setKeys.length > 0 || unsetKeys.length > 0);

    const currentRuntime = String(savedActor.runtime || "codex").trim();
    const currentCommand = formatRuntimeCommand(savedActor.command);
    const currentTitle = String(savedActor.title || "").trim();
    const currentCapabilityAutoload = normalizeCapabilityIdList(
      (savedActor as { capability_autoload?: unknown[] })?.capability_autoload,
    );
    const currentActorNotes = String(editActorNotesBaselineRef.current || "").trim();
    const nextActorNotes = String(editActorNotes || "").trim();
    const nextRuntime = String(
      mode === "profile" ? selectedProfile?.runtime || currentRuntime : editActorRuntime || "codex",
    ).trim();
    const grokBotUrl = nextRuntime === "grok_web_model" ? payload.grokBotUrl?.trim() : undefined;
    if (grokBotUrl !== undefined && !isGrokBotUrl(grokBotUrl)) {
      throw new Error(t("settings:grokActor.invalidUrl"));
    }
    const nextCommand = isWebModelRuntime(nextRuntime)
      ? currentCommand
      : String(editActorCommand || "").trim();
    const nextTitle = String(editActorTitle || "").trim();
    const currentDefaultScopeKey = savedActor.default_scope_key || "";
    const nextDefaultScopeKey = savedActor.external_session_id
      ? currentDefaultScopeKey
      : (payload.defaultScopeKey ?? currentDefaultScopeKey);
    const scopeChanged = nextDefaultScopeKey !== currentDefaultScopeKey;
    const nextCapabilityAutoload = Array.isArray(payload.capabilityAutoload)
      ? normalizeCapabilityIdList(payload.capabilityAutoload)
      : [];

    const nextRuntimeMode = supportsAcpMode(nextRuntime)
      ? payload.runtimeMode || "default"
      : "default";
    const runtimeModeChanged =
      mode === "custom" && nextRuntimeMode !== (savedActor.runtime_mode || "default");
    const runtimeChanged =
      mode === "custom" && (!linkedBefore || convertToCustom) && nextRuntime !== currentRuntime;
    const commandChanged =
      mode === "custom" && (!linkedBefore || convertToCustom) && nextCommand !== currentCommand;
    const titleChanged = nextTitle !== currentTitle;
    const autoloadChanged =
      JSON.stringify(nextCapabilityAutoload) !== JSON.stringify(currentCapabilityAutoload);
    const profileChanged =
      mode === "profile" &&
      !actorProfileMatchesRef(selectedProfile || { id: "", scope: "global", owner_id: "" }, {
        profileId: String(savedActor.profile_id || "").trim(),
        profileScope: String(savedActor.profile_scope || "global").trim() || "global",
        profileOwner: String(savedActor.profile_owner || "").trim(),
      });
    const actorNotesChanged = nextActorNotes !== currentActorNotes;
    const hasActorMutation =
      convertToCustom ||
      runtimeChanged ||
      runtimeModeChanged ||
      commandChanged ||
      titleChanged ||
      autoloadChanged ||
      scopeChanged ||
      profileChanged;

    if (
      !options.restart &&
      !hasActorMutation &&
      !willChangeSecrets &&
      !actorNotesChanged &&
      grokBotUrl === undefined
    ) {
      setEditingActor(null);
      return;
    }

    if (options.restart) {
      const msg =
        grokBotUrl !== undefined
          ? t("applyWebModelTargetConfirm", { label })
          : willChangeSecrets
            ? t("saveSecretsAndRestartConfirm", { label })
            : t("saveAndRestartConfirm", { label });
      if (!window.confirm(msg)) return;
    }

    const targetGroupId = selectedGroupId;
    const visit = ++editActorVisitRef.current;
    const isCurrentEdit = () =>
      editActorVisitRef.current === visit &&
      useGroupStore.getState().selectedGroupId === targetGroupId;
    setBusy("actor-update");
    try {
      let actorSnapshot: Record<string, unknown> = savedActor as unknown as Record<string, unknown>;

      // Runtime conversion still requires stopping the old backend. An existing
      // native Grok Actor applies its route under the daemon's admission lock.
      if (grokBotUrl !== undefined && currentRuntime !== "grok_web_model") {
        const stopResp = await api.stopActor(targetGroupId, actorId);
        if (!stopResp.ok) throw new Error(`${stopResp.error.code}: ${stopResp.error.message}`);
        actorSnapshot = { ...actorSnapshot, enabled: false, running: false };
        if (isCurrentEdit()) setEditingActor(actorSnapshot as Actor);
      }

      if (mode === "custom" && linkedBefore && convertToCustom) {
        const convertResp = await api.updateActor(
          targetGroupId,
          actorId,
          undefined,
          undefined,
          nextTitle,
          {
            profileAction: "convert_to_custom",
            capabilityAutoload: nextCapabilityAutoload,
            ...(scopeChanged ? { defaultScopeKey: nextDefaultScopeKey } : {}),
          },
        );
        if (!convertResp.ok) {
          throw new Error(`${convertResp.error.code}: ${convertResp.error.message}`);
        }
        const updated =
          convertResp.result && typeof convertResp.result === "object"
            ? (convertResp.result as { actor?: Record<string, unknown> }).actor
            : undefined;
        if (updated && typeof updated === "object") {
          actorSnapshot = updated;
          if (isCurrentEdit()) setEditingActor(updated as Actor);
        }
      }

      if (mode === "profile") {
        const needProfilePatch = profileChanged || titleChanged || autoloadChanged || scopeChanged;
        if (needProfilePatch) {
          const profileResp = await api.updateActor(
            targetGroupId,
            actorId,
            undefined,
            undefined,
            nextTitle,
            {
              profileId,
              profileScope: (selectedProfile?.scope || "global") as api.ProfileScope,
              profileOwner: String(selectedProfile?.owner_id || "").trim() || undefined,
              capabilityAutoload: nextCapabilityAutoload,
              ...(scopeChanged ? { defaultScopeKey: nextDefaultScopeKey } : {}),
            },
          );
          if (!profileResp.ok) {
            throw new Error(`${profileResp.error.code}: ${profileResp.error.message}`);
          }
          const updated =
            profileResp.result && typeof profileResp.result === "object"
              ? (profileResp.result as { actor?: Record<string, unknown> }).actor
              : undefined;
          if (updated && typeof updated === "object") {
            actorSnapshot = updated;
            if (isCurrentEdit()) setEditingActor(updated as Actor);
          }
        }
      } else {
        const snapshotRuntime = String(actorSnapshot.runtime || currentRuntime || "codex").trim();
        const snapshotCommand = formatRuntimeCommand(actorSnapshot.command);
        const snapshotTitle = String(actorSnapshot.title || "").trim();
        const snapshotScopeChanged =
          nextDefaultScopeKey !== (actorSnapshot.default_scope_key || "");
        const needCustomPatch =
          nextRuntime !== snapshotRuntime ||
          nextRuntimeMode !== (actorSnapshot.runtime_mode || "default") ||
          nextCommand !== snapshotCommand ||
          nextTitle !== snapshotTitle ||
          snapshotScopeChanged ||
          autoloadChanged;
        if (needCustomPatch) {
          const customResp = await api.updateActor(
            targetGroupId,
            actorId,
            nextRuntime !== snapshotRuntime ? editActorRuntime : undefined,
            nextCommand !== snapshotCommand ? nextCommand : undefined,
            nextTitle,
            {
              capabilityAutoload: nextCapabilityAutoload,
              ...(snapshotScopeChanged ? { defaultScopeKey: nextDefaultScopeKey } : {}),
              ...(supportsAcpMode(nextRuntime) || actorSnapshot.runtime_mode === "acp"
                ? { runtimeMode: nextRuntimeMode }
                : {}),
            },
          );
          if (!customResp.ok) {
            throw new Error(`${customResp.error.code}: ${customResp.error.message}`);
          }
          const updated =
            customResp.result && typeof customResp.result === "object"
              ? (customResp.result as { actor?: Record<string, unknown> }).actor
              : undefined;
          if (updated && typeof updated === "object") {
            actorSnapshot = updated;
            if (isCurrentEdit()) setEditingActor(updated as Actor);
          }
        }
      }

      if (willChangeSecrets) {
        const envResp = await api.updateActorPrivateEnv(
          targetGroupId,
          actorId,
          setVars,
          unsetKeys,
          clear,
        );
        if (!envResp.ok) {
          throw new Error(`${envResp.error.code}: ${envResp.error.message}`);
        }
        if (isCurrentEdit()) payload.onSecretsSaved?.(envResp.result.keys);
      }

      if (actorNotesChanged) {
        const actorNotesResp = await persistActorNotes(
          targetGroupId,
          actorId,
          nextActorNotes,
          actors.map((item) => String(item.id || "").trim()).filter(Boolean),
        );
        if (!actorNotesResp.ok) {
          throw new Error(actorNotesResp.error);
        }
        if (isCurrentEdit()) editActorNotesBaselineRef.current = nextActorNotes;
      }

      if (grokBotUrl !== undefined) {
        const bindResp = await api.bindGrokBot(targetGroupId, actorId, grokBotUrl);
        if (!bindResp.ok) throw new Error(`${bindResp.error.code}: ${bindResp.error.message}`);
      }

      if (options.restart) {
        const restartResp =
          grokBotUrl !== undefined
            ? await api.startActor(targetGroupId, actorId)
            : await api.restartActor(targetGroupId, actorId);
        if (!restartResp.ok) {
          throw new Error(`${restartResp.error.code}: ${restartResp.error.message}`);
        }
      }

      await refreshActors(targetGroupId);
      if (!isCurrentEdit()) return;
      // Saving the Bot is the first connection step; keep Routine setup reachable.
      if (grokBotUrl === undefined) setEditingActor(null);

      if (!options.restart && grokBotUrl === undefined) {
        const isRunning = Boolean(editingActor.running ?? editingActor.enabled ?? false);
        const restartRequired =
          isRunning &&
          (willChangeSecrets ||
            profileChanged ||
            runtimeChanged ||
            commandChanged ||
            scopeChanged ||
            convertToCustom);
        if (restartRequired) {
          showNotice({ message: t("savedRestartRequired", { label }) });
        }
      }
    } finally {
      if (useUIStore.getState().busy === "actor-update") setBusy("");
    }
  };

  const handleSaveEditActorOnly = async (payload: EditActorSavePayload) => {
    await handleSaveEditActor(payload, { restart: false });
  };

  const handleSaveEditActorAndRestart = async (payload: EditActorSavePayload) => {
    await handleSaveEditActor(payload, { restart: true });
  };

  useEffect(() => {
    if (!editingActorId || !selectedGroupId) return;
    void loadEditingActorNotes(selectedGroupId, editingActorId);
  }, [editingActorId, selectedGroupId, loadEditingActorNotes]);

  useEffect(() => {
    if (!editingActor) return;
    const latest = actors.find((item) => item.id === editingActor.id);
    if (!latest) return;
    // Polls may observe an intermediate save. Only refresh the independently
    // saved avatar; runtime fields and secret edits belong to the open draft.
    if (
      editingActor.avatar_url !== latest.avatar_url ||
      editingActor.has_custom_avatar !== latest.has_custom_avatar
    ) {
      setEditingActor({
        ...editingActor,
        avatar_url: latest.avatar_url,
        has_custom_avatar: latest.has_custom_avatar,
      });
    }
  }, [actors, editingActor, setEditingActor]);

  const handleSaveEditActorAsProfile = async (
    secrets?: ActorSecretSaveChanges,
    runtimeMode?: "default" | "acp",
  ): Promise<SaveActorProfileResult | void> => {
    if (!editingActor || !selectedGroupId) return;
    const suggested = String(
      editActorTitle || editingActor.title || editingActor.id || "New Profile",
    ).trim();
    const name =
      editProfileSaveRef.current?.profile.name || window.prompt(t("profileNamePrompt"), suggested);
    if (!name || !name.trim()) return;
    setBusy("actor-profile-save");
    try {
      const resp = await api.upsertActorProfile(
        {
          id: editProfileSaveRef.current?.profile.id,
          name: name.trim(),
          runtime: editActorRuntime,
          ...(supportsAcpMode(editActorRuntime) ? { runtime_mode: runtimeMode || "default" } : {}),
          command: isWebModelRuntime(editActorRuntime) ? "" : editActorCommand.trim(),
          submit: String(editingActor.submit || "enter"),
          env: {},
          capability_defaults: {
            autoload_capabilities: parseCapabilityIdInput(editActorCapabilityAutoloadText),
            default_scope: "actor",
            session_ttl_seconds: 3600,
          },
        },
        editProfileSaveRef.current?.profile.revision,
      );
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const profileId = String(resp.result?.profile?.id || "").trim();
      if (profileId) {
        editProfileSaveRef.current = {
          profile: resp.result.profile,
          copied: editProfileSaveRef.current?.copied || false,
        };
      }
      if (
        profileId &&
        !isWebModelRuntime(editActorRuntime) &&
        !editProfileSaveRef.current?.copied
      ) {
        const copyResp = await api.copyActorPrivateEnvToProfile(
          profileId,
          selectedGroupId,
          editingActor.id,
        );
        if (!copyResp.ok) {
          showError(`${copyResp.error.code}: ${copyResp.error.message}`);
          return;
        }
        if (editProfileSaveRef.current) editProfileSaveRef.current.copied = true;
      }
      if (
        profileId &&
        !isWebModelRuntime(editActorRuntime) &&
        secrets &&
        (secrets.clear || secrets.unsetKeys.length || Object.keys(secrets.setVars).length)
      ) {
        const secretResp = await api.updateActorProfilePrivateEnv(
          profileId,
          secrets.setVars,
          secrets.unsetKeys,
          secrets.clear,
        );
        if (!secretResp.ok) {
          showError(`${secretResp.error.code}: ${secretResp.error.message}`);
          return;
        }
      }
      await loadActorProfiles();
      const profileName = String(resp.result?.profile?.name || "").trim() || name.trim();
      editProfileSaveRef.current = null;
      showNotice({ message: t("savedToActorProfiles") });
      if (!profileId) return;
      const useNow = window.confirm(
        t("useSavedProfileNowConfirm", {
          name: profileName,
          actor:
            String(editActorTitle || editingActor.title || editingActor.id || "").trim() ||
            editingActor.id,
        }),
      );
      return { profileId, profileName, useNow };
    } finally {
      setBusy("");
    }
  };

  const suggestedActorId = (() => {
    const selectedProfile =
      actorProfiles.find(
        (item) => actorProfileIdentityKey(item) === String(newActorProfileId || "").trim(),
      ) || null;
    const profileRuntime = String(selectedProfile?.runtime || "").trim();
    const prefix = newActorUseProfile
      ? profileRuntime || "actor"
      : isWebModelRuntime(newActorRuntime)
        ? newActorRuntime === "chatgpt_dots"
          ? "chatgpt-dots"
          : newActorRuntime === "grok_web_model"
            ? "grok-web"
            : "chatgpt-web"
        : newActorRuntime;
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
    if (isWebModelRuntime(newActorRuntime)) return true;
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = runtimeDetectionStatus === "ready" && (rtInfo?.available ?? false);
    if (!newActorUseDefaultCommand && !newActorCommand.trim()) return false;
    if (newActorRuntime === "custom" && (newActorUseDefaultCommand || !newActorCommand.trim()))
      return false;
    if (
      !available &&
      newActorRuntime !== "antigravity" &&
      (newActorUseDefaultCommand || !newActorCommand.trim())
    )
      return false;
    return true;
  })();

  const addActorDisabledReason = (() => {
    if (busy === "actor-add") return "";
    if (newActorUseProfile && !String(newActorProfileId || "").trim()) {
      return t("profileRequired");
    }
    if (isWebModelRuntime(newActorRuntime)) return "";
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = runtimeDetectionStatus === "ready" && (rtInfo?.available ?? false);
    if (!newActorUseDefaultCommand && !newActorCommand.trim()) {
      return t("commandOverrideRequired");
    }
    if (newActorRuntime === "custom" && (newActorUseDefaultCommand || !newActorCommand.trim())) {
      return t("customRuntimeRequiresCommand");
    }
    if (
      !available &&
      newActorRuntime !== "antigravity" &&
      (newActorUseDefaultCommand || !newActorCommand.trim())
    ) {
      const key = runtimeDetectionKey(newActorRuntime, rtInfo, runtimeDetectionStatus);
      return key === "missing"
        ? t("runtimeDetection.missingRuntime", {
            runtime: RUNTIME_INFO[newActorRuntime]?.label || newActorRuntime,
          })
        : t(`runtimeDetection.${key}`);
    }
    return "";
  })();

  const handleAddActor = async (
    avatarFile?: File | null,
    runtimeMode?: "default" | "acp",
    defaultScopeKey = "",
  ): Promise<boolean> => {
    if (!selectedGroupId) return false;
    const creationGroupId = selectedGroupId;
    const creationActiveTab = useUIStore.getState().activeTab;
    const actorId = resolveNewActorId(newActorId, suggestedActorId);
    const secretsText = String(newActorSecretsSetText || "");
    const actorNotes = String(newActorNotes || "").trim();
    const selectedProfile =
      actorProfiles.find(
        (item) => actorProfileIdentityKey(item) === String(newActorProfileId || "").trim(),
      ) || null;
    const capabilityAutoload = parseCapabilityIdInput(newActorCapabilityAutoloadText);

    if (newActorUseProfile && !selectedProfile) {
      setAddActorError(t("selectProfileFirst"));
      return false;
    }

    let secretsSetVars: Record<string, string> = {};
    if (!newActorUseProfile && !isWebModelRuntime(newActorRuntime)) {
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
      const commandToUse =
        newActorUseProfile || isWebModelRuntime(newActorRuntime)
          ? ""
          : newActorUseDefaultCommand
            ? ""
            : newActorCommand;
      const resp = await api.addActor(
        selectedGroupId,
        actorId,
        newActorRole,
        newActorUseProfile ? String(selectedProfile?.runtime || "codex") : newActorRuntime,
        commandToUse,
        newActorUseProfile
          ? undefined
          : Object.keys(secretsSetVars).length
            ? secretsSetVars
            : undefined,
        newActorUseProfile
          ? {
              profileId: String(selectedProfile?.id || "").trim(),
              profileScope: (selectedProfile?.scope || "global") as api.ProfileScope,
              profileOwner: String(selectedProfile?.owner_id || "").trim() || undefined,
              capabilityAutoload,
              defaultScopeKey,
            }
          : {
              capabilityAutoload,
              defaultScopeKey,
              ...(supportsAcpMode(newActorRuntime) ? { runtimeMode } : {}),
            },
      );
      if (!resp.ok) {
        setAddActorError(resp.error?.message || t("failedToAddAgent"));
        return false;
      }

      const createdActorId = String(
        (resp.result && typeof resp.result === "object"
          ? (resp.result as { actor?: { id?: string } }).actor?.id
          : "") ||
          actorId ||
          suggestedActorId,
      ).trim();

      const postCreateErrors: string[] = [];

      if (actorNotes && createdActorId) {
        const actorNotesResp = await persistActorNotes(
          selectedGroupId,
          createdActorId,
          actorNotes,
          [...actors.map((item) => String(item.id || "").trim()).filter(Boolean), createdActorId],
        );
        if (!actorNotesResp.ok) {
          postCreateErrors.push(`${t("actorNotes")}: ${actorNotesResp.error}`);
        }
      }

      if (avatarFile && createdActorId) {
        const avatarResp = await api.uploadActorAvatar(selectedGroupId, createdActorId, avatarFile);
        if (!avatarResp.ok) {
          postCreateErrors.push(
            `${t("avatarTitle")}: ${avatarResp.error?.message || t("avatarUploadFailed")}`,
          );
        }
      }

      const createdRuntime = (resp.result as { actor?: { runtime?: string } })?.actor?.runtime;
      if (
        (createdRuntime === "chatgpt_dots" || createdRuntime === "grok_web_model") &&
        useGroupStore.getState().selectedGroupId === creationGroupId &&
        useUIStore.getState().activeTab === creationActiveTab
      ) {
        onNativeActorCreated?.({
          groupId: creationGroupId,
          actorId: createdActorId,
          activeTab: creationActiveTab,
          runtime: createdRuntime,
        });
      }
      closeModal("addActor");
      resetAddActorForm();
      await refreshActors(creationGroupId);
      if (postCreateErrors.length > 0) {
        showError(
          t("actorCreatedSetupFailed", {
            actor: createdActorId,
            details: postCreateErrors.join(" · "),
          }),
        );
      }
      return true;
    } finally {
      setBusy("");
    }
  };

  const handleSaveNewActorAsProfile = async (
    _secrets?: ActorSecretSaveChanges,
    runtimeMode?: "default" | "acp",
  ) => {
    if (newActorUseProfile) return;
    const parsed = parsePrivateEnvSetText(
      isWebModelRuntime(newActorRuntime) ? "" : newActorSecretsSetText,
    );
    if (!parsed.ok) {
      setAddActorError(parsed.error);
      return;
    }
    const suggested = String(newActorId || `${newActorRuntime}-profile`).trim();
    const name =
      newProfileSaveRef.current?.name || window.prompt(t("profileNamePrompt"), suggested);
    if (!name || !name.trim()) return;
    setBusy("actor-profile-save");
    try {
      const commandToUse =
        newActorUseDefaultCommand || isWebModelRuntime(newActorRuntime)
          ? ""
          : newActorCommand.trim();
      const resp = await api.upsertActorProfile(
        {
          id: newProfileSaveRef.current?.id,
          name: name.trim(),
          runtime: newActorRuntime,
          ...(supportsAcpMode(newActorRuntime) ? { runtime_mode: runtimeMode || "default" } : {}),
          command: commandToUse,
          submit: "enter",
          env: {},
          capability_defaults: {
            autoload_capabilities: parseCapabilityIdInput(newActorCapabilityAutoloadText),
            default_scope: "actor",
            session_ttl_seconds: 3600,
          },
        },
        newProfileSaveRef.current?.revision,
      );
      if (!resp.ok) {
        setAddActorError(resp.error?.message || t("failedToSaveActorProfile"));
        return;
      }
      const profileId = String(resp.result?.profile?.id || "").trim();
      if (profileId) {
        newProfileSaveRef.current = resp.result.profile;
        const hasSecrets = Object.keys(parsed.setVars).length > 0;
        if (hasSecrets) {
          const secretResp = await api.updateActorProfilePrivateEnv(
            profileId,
            parsed.setVars,
            [],
            false,
          );
          if (!secretResp.ok) {
            setAddActorError(secretResp.error?.message || t("failedToSaveActorProfile"));
            return;
          }
        }
      }
      newProfileSaveRef.current = null;
      showNotice({ message: t("savedToActorProfiles") });
      await loadActorProfiles();
    } finally {
      setBusy("");
    }
  };

  const handleCloseAddActor = useCallback(() => {
    closeModal("addActor");
    resetAddActorForm();
  }, [closeModal, resetAddActorForm]);

  const handleCancelEditActor = useCallback(() => {
    editActorVisitRef.current += 1;
    editActorNotesSeqRef.current += 1;
    editActorNotesBaselineRef.current = "";
    setEditActorNotesBusy(false);
    setEditActorNotes("");
    setEditingActor(null);
  }, [setEditActorNotes, setEditingActor]);

  return {
    actorProfiles,
    actorProfilesBusy,
    editActorNotesBusy,
    savedActorNotes: editActorNotesBaselineRef.current,
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
  };
}
