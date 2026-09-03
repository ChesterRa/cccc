import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  buildActorSecretSaveChanges,
  emptyActorSecretChanges,
  type ActorSecretChanges,
} from "../../components/modals/actorSecretManagerModel";
import {
  copyVoiceAnalystPrivateEnvToProfile,
  fetchCodexVoiceAnalystSettings,
  listActorProfiles,
  upsertActorProfile,
  updateCodexVoiceAnalystSettings,
  updateProfilePrivateEnv,
} from "../../services/api";
import type { ActorProfile } from "../../types";
import { actorProfileIdentityKey, actorProfileMatchesRef } from "../../utils/actorProfiles";
import type { RuntimeConfigurationMode } from "../../components/modals/RuntimeProfileControls";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import {
  analystIdentityEnvironmentKeys,
  bindVoiceAnalystProfile,
  defaultAnalystRuntimeCommand,
  emptyVoiceAnalystSettings,
  managedAnalystRuntimes,
  normalizeVoiceAnalystSettings,
  type VoiceAnalystDraftSettings,
} from "./codexVoiceAnalystSettingsModel";

export function useCodexVoiceAnalystSettings(
  active: boolean,
  controller: CodexVoiceSessionController,
) {
  const { t } = useTranslation("modals");
  const { t: tActors } = useTranslation("actors");
  const [settings, setSettings] = useState<VoiceAnalystDraftSettings>(emptyVoiceAnalystSettings);
  const [loadedSettings, setLoadedSettings] =
    useState<VoiceAnalystDraftSettings>(emptyVoiceAnalystSettings);
  const [mode, setMode] = useState<RuntimeConfigurationMode>("custom");
  const [profiles, setProfiles] = useState<ActorProfile[]>([]);
  const [environmentKeys, setEnvironmentKeys] = useState<string[]>([]);
  const [environmentChanges, setEnvironmentChangesState] =
    useState<ActorSecretChanges>(emptyActorSecretChanges);
  const [settingsLoadFailed, setSettingsLoadFailed] = useState(false);
  const [profilesLoadFailed, setProfilesLoadFailed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [environmentRefreshing, setEnvironmentRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [profileSaving, setProfileSaving] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");

  const settingsError = useCallback(
    (code: string, detail: string) => {
      if (code === "codex_voice_settings_busy") return t("codexVoiceAnalystSettingsBusy");
      if (code === "codex_voice_settings_unavailable") {
        return t("codexVoiceAnalystSettingsUnavailable");
      }
      if (code === "codex_voice_settings_invalid") {
        return t("codexVoiceAnalystSettingsInvalid", { detail });
      }
      return detail;
    },
    [t],
  );

  const load = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    const [settingsResponse, profilesResponse] = await Promise.all([
      fetchCodexVoiceAnalystSettings(),
      listActorProfiles(),
    ]);
    if (settingsResponse.ok) {
      const nextSettings = normalizeVoiceAnalystSettings(settingsResponse.result.settings);
      setSettings(nextSettings);
      setLoadedSettings(nextSettings);
      setMode(nextSettings.profile_id ? "profile" : "custom");
      setEnvironmentKeys(settingsResponse.result.environment_keys);
      setEnvironmentChangesState(emptyActorSecretChanges());
      setSettingsLoadFailed(false);
      setError("");
    } else {
      setSettingsLoadFailed(true);
      setError(settingsError(settingsResponse.error.code, settingsResponse.error.message));
    }
    if (profilesResponse.ok) {
      setProfiles(profilesResponse.result.profiles);
      setProfilesLoadFailed(false);
    } else {
      setProfiles([]);
      setProfilesLoadFailed(true);
      if (settingsResponse.ok) setError(t("codexVoiceAnalystProfilesUnavailable"));
    }
    setLoading(false);
  }, [active, settingsError, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const compatibleProfiles = useMemo(
    () =>
      profiles.filter((profile) =>
        managedAnalystRuntimes.has(String(profile.runtime).trim().toLowerCase()),
      ),
    [profiles],
  );
  const profileIdentity = settings.profile_id
    ? actorProfileIdentityKey({
        id: settings.profile_id,
        scope: settings.profile_scope,
        owner_id: settings.profile_owner,
      })
    : "";
  const selectedProfile = compatibleProfiles.find((profile) =>
    actorProfileMatchesRef(profile, {
      profileId: settings.profile_id,
      profileScope: settings.profile_scope,
      profileOwner: settings.profile_owner,
    }),
  );
  const environmentSaveChanges = useMemo(
    () => buildActorSecretSaveChanges(environmentChanges),
    [environmentChanges],
  );
  const hasEnvironmentChanges =
    mode === "custom" &&
    (environmentSaveChanges.clear ||
      environmentSaveChanges.unsetKeys.length > 0 ||
      Object.keys(environmentSaveChanges.setVars).length > 0);
  const hasChanges =
    JSON.stringify(settings) !== JSON.stringify(loadedSettings) || hasEnvironmentChanges;
  const blocked = controller.isEngaged || controller.analyst?.phase === "working";
  const editingDisabled = loading || saving || profileSaving;
  const profileInvalid = mode === "profile" && (!selectedProfile || profilesLoadFailed);
  const saveDisabled =
    blocked || editingDisabled || settingsLoadFailed || profileInvalid || !hasChanges;

  const changeMode = (nextMode: RuntimeConfigurationMode) => {
    setMode(nextMode);
    setSaved("");
    setError("");
    if (nextMode === "custom") {
      setSettings((current) => bindVoiceAnalystProfile(current));
      return;
    }
    setEnvironmentChangesState(emptyActorSecretChanges());
    setSettings((current) =>
      bindVoiceAnalystProfile(current, selectedProfile || compatibleProfiles[0]),
    );
  };

  const selectProfile = (identity: string) => {
    const profile = compatibleProfiles.find(
      (candidate) => actorProfileIdentityKey(candidate) === identity,
    );
    setSettings((current) => bindVoiceAnalystProfile(current, profile));
    setSaved("");
  };

  const refreshEnvironment = async () => {
    setEnvironmentRefreshing(true);
    const response = await fetchCodexVoiceAnalystSettings();
    if (response.ok) {
      if (settingsLoadFailed) {
        const nextSettings = normalizeVoiceAnalystSettings(response.result.settings);
        setSettings(nextSettings);
        setLoadedSettings(nextSettings);
        setMode(nextSettings.profile_id ? "profile" : "custom");
        setEnvironmentChangesState(emptyActorSecretChanges());
        setSettingsLoadFailed(false);
      }
      setEnvironmentKeys(response.result.environment_keys);
      setError("");
    } else {
      setError(settingsError(response.error.code, response.error.message));
    }
    setEnvironmentRefreshing(false);
  };

  const save = async () => {
    if (saveDisabled) return;
    const identityCandidates = environmentSaveChanges.clear
      ? environmentKeys
      : [...Object.keys(environmentSaveChanges.setVars), ...environmentSaveChanges.unsetKeys];
    const changesAnalystIdentity =
      settings.runtime !== loadedSettings.runtime ||
      settings.profile_id !== loadedSettings.profile_id ||
      settings.profile_scope !== loadedSettings.profile_scope ||
      settings.profile_owner !== loadedSettings.profile_owner ||
      (mode === "custom" &&
        identityCandidates.some((key) => analystIdentityEnvironmentKeys.has(key)));
    if (
      changesAnalystIdentity &&
      controller.analyst?.tui_ready &&
      !window.confirm(t("codexVoiceAnalystIdentityChangeConfirm"))
    ) {
      return;
    }
    setSaving(true);
    setError("");
    setSaved("");
    const response = await updateCodexVoiceAnalystSettings({
      settings,
      environmentSet: mode === "custom" ? environmentSaveChanges.setVars : {},
      environmentUnset: mode === "custom" ? environmentSaveChanges.unsetKeys : [],
      environmentClear: mode === "custom" && environmentSaveChanges.clear,
    });
    if (response.ok) {
      setSaved(
        response.result.started_new_session
          ? t("codexVoiceAnalystSettingsNewSession")
          : response.result.restarted
            ? t("codexVoiceAnalystSettingsRestarted")
            : t("codexVoiceAnalystSettingsSaved"),
      );
      await load();
      await controller.refresh(false);
    } else {
      setError(settingsError(response.error.code, response.error.message));
    }
    setSaving(false);
  };

  const setCommand = (command: string) => {
    setSettings((current) => ({ ...current, command }));
    setSaved("");
  };
  const setRuntime = (runtime: string) => {
    if (!managedAnalystRuntimes.has(runtime)) return;
    setSettings((current) => {
      const previousDefault = defaultAnalystRuntimeCommand(current.runtime);
      const command =
        !current.command.trim() || current.command.trim() === previousDefault
          ? ""
          : current.command;
      return { ...current, runtime, command };
    });
    setSaved("");
    setError("");
  };
  const defaultCommand = defaultAnalystRuntimeCommand(settings.runtime);
  // An empty command is the persisted "use the runtime default" sentinel.
  // Once the user opts out, keep the copied default as an explicit editable
  // command instead of inferring the checkbox state from string equality.
  const useDefaultCommand = !settings.command.trim();
  const setUseDefaultCommand = (enabled: boolean) => {
    setCommand(enabled ? "" : settings.command.trim() || defaultCommand);
  };

  const saveAsProfile = async () => {
    if (mode !== "custom" || editingDisabled || settingsLoadFailed) return;
    const name = window.prompt(tActors("profileNamePrompt"), "Voice Analyst");
    if (!name?.trim()) return;
    setProfileSaving(true);
    setError("");
    setSaved("");
    try {
      const response = await upsertActorProfile({
        name: name.trim(),
        runtime: settings.runtime,
        runner: "pty",
        command: settings.command.trim(),
        submit: "enter",
        env: {},
      });
      if (!response.ok) {
        setError(t("codexVoiceAnalystProfileSaveFailed", { detail: response.error.message }));
        return;
      }
      const profile = response.result.profile;
      const profileId = String(profile?.id || "").trim();
      if (!profileId) {
        setError(t("codexVoiceAnalystProfileSaveFailed", { detail: "profile id is missing" }));
        return;
      }
      const copyResponse = await copyVoiceAnalystPrivateEnvToProfile(profileId);
      if (!copyResponse.ok) {
        setError(t("codexVoiceAnalystProfileSaveFailed", { detail: copyResponse.error.message }));
        return;
      }
      if (
        environmentSaveChanges.clear ||
        environmentSaveChanges.unsetKeys.length > 0 ||
        Object.keys(environmentSaveChanges.setVars).length > 0
      ) {
        const environmentResponse = await updateProfilePrivateEnv(
          profileId,
          environmentSaveChanges.setVars,
          environmentSaveChanges.unsetKeys,
          environmentSaveChanges.clear,
          { scope: "global", ownerId: "" },
        );
        if (!environmentResponse.ok) {
          setError(
            t("codexVoiceAnalystProfileSaveFailed", { detail: environmentResponse.error.message }),
          );
          return;
        }
      }
      setProfiles((current) => [
        ...current.filter(
          (candidate) => actorProfileIdentityKey(candidate) !== actorProfileIdentityKey(profile),
        ),
        profile,
      ]);
      setMode("profile");
      setSettings((current) => bindVoiceAnalystProfile(current, profile));
      setEnvironmentChangesState(emptyActorSecretChanges());
      setSaved(t("codexVoiceAnalystProfileCreated", { name: profile.name || name.trim() }));
    } catch (error) {
      setError(
        t("codexVoiceAnalystProfileSaveFailed", {
          detail: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setProfileSaving(false);
    }
  };
  const setEnvironmentChanges = (changes: ActorSecretChanges) => {
    setEnvironmentChangesState(changes);
    setSaved("");
  };

  return {
    settings,
    mode,
    compatibleProfiles,
    profileIdentity,
    environmentKeys,
    environmentChanges,
    loading,
    environmentRefreshing,
    settingsLoadFailed,
    saving,
    profileSaving,
    error,
    saved,
    blocked,
    editingDisabled,
    saveDisabled,
    changeMode,
    selectProfile,
    refreshEnvironment,
    setCommand,
    setRuntime,
    defaultCommand,
    useDefaultCommand,
    setUseDefaultCommand,
    setEnvironmentChanges,
    saveAsProfile,
    save,
  };
}
