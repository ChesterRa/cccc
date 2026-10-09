import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { supportsAcpMode, type ActorProfile } from "../../../types";
import { actorProfileIdentityKey, actorProfileMatchesRef } from "../../../utils/actorProfiles";
import { formatRuntimeCommand } from "../runtimeProfileControlsModel";
import {
  buildActorSecretSaveChanges,
  emptyActorSecretChanges,
  type ActorSecretChanges,
} from "../actorSecretManagerModel";
import {
  bindVoiceAnalystProfile,
  defaultAnalystRuntimeCommand,
  managedAnalystRuntimes,
  type VoiceAnalystDraftSettings,
} from "../../../features/codexVoice/codexVoiceAnalystSettingsModel";
import {
  saveVoiceRuntimeProfile,
  type VoiceProfileCheckpoint,
} from "../../../features/voice/saveVoiceRuntimeProfile";

export const defaultSecretaryPreferences: api.SecretaryPreferences = {
  recognition_backend: "browser_asr",
  recognition_language: "auto",
  external_asr_provider: "bailian",
  service_model_id: "",
  service_diarization_model_id: "",
  auto_document_max_window_seconds: 300,
  guidance: "",
};
function draft(value: api.GlobalVoiceSecretarySettings): VoiceAnalystDraftSettings {
  return {
    runtime: value.runtime || "",
    runtime_mode: value.runtime_mode || "default",
    command: formatRuntimeCommand(value.command),
    profile_id: value.profile_id || "",
    profile_scope: value.profile_scope === "user" ? "user" : "global",
    profile_owner: value.profile_owner || "",
  };
}
const emptyRuntime = draft({ profile_id: "", config: defaultSecretaryPreferences });

export function useVoiceSecretarySettings(active: boolean) {
  const { t } = useTranslation("settings");
  const { t: tActors } = useTranslation("actors");
  const [state, setState] = useState<api.GlobalVoiceSecretaryState | null>(null);
  const [settings, setSettings] = useState(emptyRuntime);
  const [loadedSettings, setLoadedSettings] = useState(emptyRuntime);
  const [preferences, setPreferences] = useState(defaultSecretaryPreferences);
  const [loadedPreferences, setLoadedPreferences] = useState(defaultSecretaryPreferences);
  const preferenceBaseline = useRef(defaultSecretaryPreferences);
  const [mode, setMode] = useState<"custom" | "profile">("custom");
  const [environmentChanges, setEnvironmentChanges] =
    useState<ActorSecretChanges>(emptyActorSecretChanges);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [profileSaving, setProfileSaving] = useState(false);
  const profileSaveRef = useRef<VoiceProfileCheckpoint | null>(null);
  const [preferencesSaving, setPreferencesSaving] = useState(false);
  const preferenceWrite = useRef(false);
  const [preferencesError, setPreferencesError] = useState("");
  const failedPreferenceFields = useRef<string[]>([]);
  const preferenceFailure = useRef("");
  const [preferencesNotice, setPreferencesNotice] = useState("");
  const [settingsLoadFailed, setSettingsLoadFailed] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [backlogAction, setBacklogAction] = useState<"process" | "hold" | "">("");
  const sequence = useRef(0);
  const invalidateLoads = useCallback(() => {
    sequence.current++;
  }, []);
  const dirty = useRef(false);
  const environmentSaveChanges = useMemo(
    () => buildActorSecretSaveChanges(environmentChanges),
    [environmentChanges],
  );
  const hasEnvironmentChanges =
    mode === "custom" &&
    (environmentSaveChanges.clear ||
      environmentSaveChanges.unsetKeys.length > 0 ||
      Object.keys(environmentSaveChanges.setVars).length > 0);
  const runtimeChanged =
    JSON.stringify(settings) !== JSON.stringify(loadedSettings) || hasEnvironmentChanges;
  const preferencesChanged = JSON.stringify(preferences) !== JSON.stringify(loadedPreferences);
  const rulesChanged = preferences.guidance !== loadedPreferences.guidance;
  const hasChanges = runtimeChanged || preferencesChanged || !!backlogAction;
  dirty.current = hasChanges;

  // Update persisted baselines independently. A successful preference write must
  // never initialize the Runtime/secret draft or erase another unsaved preference.
  const syncPreferences = useCallback((next: api.SecretaryPreferences, written: string[] = []) => {
    const previous = preferenceBaseline.current;
    preferenceBaseline.current = next;
    setLoadedPreferences(next);
    setPreferences(
      (current) =>
        Object.fromEntries(
          Object.entries(next).map(([key, value]) => [
            key,
            written.includes(key) ||
            current[key as keyof api.SecretaryPreferences] ===
              previous[key as keyof api.SecretaryPreferences]
              ? value
              : current[key as keyof api.SecretaryPreferences],
          ]),
        ) as api.SecretaryPreferences,
    );
  }, []);
  const applySavedRuntime = useCallback(
    (next: api.GlobalVoiceSecretaryState) => {
      const runtime = draft(next.settings);
      setState(next);
      setSettings(runtime);
      setLoadedSettings(runtime);
      setMode(runtime.profile_id ? "profile" : "custom");
      setEnvironmentChanges(emptyActorSecretChanges());
      setBacklogAction("");
      setSettingsLoadFailed(false);
      syncPreferences({ ...defaultSecretaryPreferences, ...next.settings.config });
      if (profileSaveRef.current) profileSaveRef.current.copied = false;
    },
    [syncPreferences],
  );
  const load = useCallback(async () => {
    const seq = ++sequence.current;
    setLoading(true);
    try {
      const response = await api.fetchGlobalVoiceSecretary();
      if (seq !== sequence.current) return;
      if (response.ok) {
        if (!dirty.current) applySavedRuntime(response.result);
        else {
          setState(response.result);
          syncPreferences({ ...defaultSecretaryPreferences, ...response.result.settings.config });
        }
        if (profileSaveRef.current) profileSaveRef.current.copied = false;
        setSettingsLoadFailed(false);
        setError("");
      } else {
        setSettingsLoadFailed(true);
        setError(response.error.message);
      }
    } catch {
      if (seq === sequence.current) {
        setSettingsLoadFailed(true);
        setError(t("voiceSettings.loadFailed"));
      }
    } finally {
      if (seq === sequence.current) setLoading(false);
    }
  }, [applySavedRuntime, syncPreferences, t]);
  useEffect(() => {
    if (active) void load();
    return invalidateLoads;
  }, [active, load, invalidateLoads]);
  const compatibleProfiles = state?.profiles || [];
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
  const configured = mode === "profile" ? !!selectedProfile : !!settings.runtime;
  const needsBacklogChoice = configured && runtimeChanged && !!state?.backlog_sources;
  const editingDisabled = loading || saving || profileSaving || preferencesSaving;
  const preferencesDisabled = editingDisabled || settingsLoadFailed;
  const saveDisabled =
    !state ||
    editingDisabled ||
    settingsLoadFailed ||
    !(runtimeChanged || backlogAction) ||
    (mode === "profile" && !selectedProfile) ||
    (needsBacklogChoice && !backlogAction);
  const changed = () => {
    setNotice("");
    setError("");
  };
  const changeMode = (value: "custom" | "profile") => {
    setMode(value);
    setSettings((current) =>
      bindVoiceAnalystProfile(
        current,
        value === "profile" ? selectedProfile || compatibleProfiles[0] : undefined,
      ),
    );
    changed();
  };
  const selectProfile = (identity: string) => {
    const profile = compatibleProfiles.find(
      (candidate) => actorProfileIdentityKey(candidate) === identity,
    );
    setSettings((current) => bindVoiceAnalystProfile(current, profile));
    changed();
  };
  const acceptProfile = (profile: ActorProfile) => {
    sequence.current++;
    setState(
      (current) =>
        current && {
          ...current,
          profiles: [
            ...current.profiles.filter(
              (candidate) =>
                actorProfileIdentityKey(candidate) !== actorProfileIdentityKey(profile),
            ),
            profile,
          ],
        },
    );
    setMode("profile");
    setSettings((current) => bindVoiceAnalystProfile(current, profile));
    changed();
  };
  const setRuntime = (runtime: string) => {
    if (runtime && !managedAnalystRuntimes.has(runtime)) return;
    setSettings((current) => ({
      ...current,
      runtime,
      runtime_mode: supportsAcpMode(runtime) ? "acp" : "default",
      command:
        !runtime || current.command.trim() === defaultAnalystRuntimeCommand(current.runtime)
          ? ""
          : current.command,
    }));
    changed();
  };
  const setCommand = (command: string) => {
    setSettings((current) => ({ ...current, command }));
    changed();
  };
  const defaultCommand = settings.runtime ? defaultAnalystRuntimeCommand(settings.runtime) : "";
  const useDefaultCommand = !settings.command.trim();
  const setUseDefaultCommand = (enabled: boolean) =>
    setCommand(enabled ? "" : settings.command.trim() || defaultCommand);
  const updatePreferences = (values: Partial<api.SecretaryPreferences>) => {
    setPreferences((current) => ({ ...current, ...values }));
    setPreferencesNotice("");
  };
  const savePreferences = async (values: Partial<api.SecretaryPreferences>) => {
    if (preferencesDisabled || preferenceWrite.current) return;
    updatePreferences(values);
    sequence.current++;
    preferenceWrite.current = true;
    setPreferencesSaving(true);
    setPreferencesError("");
    try {
      const response = await api.saveGlobalVoiceSecretaryPreferences(values);
      if (response.ok) {
        setState(response.result);
        syncPreferences(
          { ...defaultSecretaryPreferences, ...response.result.settings.config },
          Object.keys(values),
        );
        failedPreferenceFields.current = failedPreferenceFields.current.filter(
          (key) => !(key in values),
        );
        setPreferencesError(failedPreferenceFields.current.length ? preferenceFailure.current : "");
        if (!failedPreferenceFields.current.length)
          setPreferencesNotice(t("voiceSettings.preferencesSaved"));
      } else {
        failedPreferenceFields.current = Array.from(
          new Set([...failedPreferenceFields.current, ...Object.keys(values)]),
        );
        preferenceFailure.current = response.error.message;
        setPreferencesError(preferenceFailure.current);
      }
    } catch {
      failedPreferenceFields.current = Array.from(
        new Set([...failedPreferenceFields.current, ...Object.keys(values)]),
      );
      preferenceFailure.current = t("voiceSettings.saveFailed");
      setPreferencesError(preferenceFailure.current);
    } finally {
      preferenceWrite.current = false;
      setPreferencesSaving(false);
    }
  };
  const retryPreferences = () => {
    const values = Object.fromEntries(
      Object.entries(preferences).filter(([key]) => failedPreferenceFields.current.includes(key)),
    ) as Partial<api.SecretaryPreferences>;
    return savePreferences(values);
  };
  const discardRules = () => {
    updatePreferences({ guidance: loadedPreferences.guidance });
    failedPreferenceFields.current = failedPreferenceFields.current.filter(
      (key) => key !== "guidance",
    );
    setPreferencesError(failedPreferenceFields.current.length ? preferenceFailure.current : "");
  };
  const discard = () => {
    profileSaveRef.current = null;
    setSettings(loadedSettings);
    setMode(loadedSettings.profile_id ? "profile" : "custom");
    setEnvironmentChanges(emptyActorSecretChanges());
    setBacklogAction("");
    changed();
  };
  const save = async () => {
    if (saveDisabled || preferenceWrite.current) return;
    sequence.current++;
    setSaving(true);
    changed();
    try {
      // Config is omitted deliberately: the server retains the latest independent preferences.
      const response = await api.saveGlobalVoiceSecretary(
        { ...settings, runtime: settings.runtime || null },
        {
          ...(configured && backlogAction ? { backlog_action: backlogAction } : {}),
          ...(mode === "custom"
            ? {
                environment: {
                  set: environmentSaveChanges.setVars,
                  unset: environmentSaveChanges.unsetKeys,
                  clear: environmentSaveChanges.clear,
                },
              }
            : {}),
        },
      );
      if (response.ok) {
        applySavedRuntime(response.result);
        setNotice(t("voiceSettings.saved"));
      } else {
        setError(response.error.message);
        if (response.error.code === "secretary_backlog_choice_required") {
          const refreshed = await api.fetchGlobalVoiceSecretary();
          if (refreshed.ok) setState(refreshed.result);
        }
      }
    } catch {
      setError(t("voiceSettings.saveFailed"));
    } finally {
      setSaving(false);
    }
  };
  const saveAsProfile = async () => {
    if (mode !== "custom" || editingDisabled || settingsLoadFailed || !settings.runtime) return;
    const name =
      profileSaveRef.current?.profile.name ||
      window.prompt(tActors("profileNamePrompt"), "Voice Secretary");
    if (!name?.trim()) return;
    sequence.current++;
    setProfileSaving(true);
    changed();
    try {
      const profile = await saveVoiceRuntimeProfile(
        name,
        settings,
        environmentSaveChanges,
        profileSaveRef,
        api.copyVoiceSecretaryPrivateEnvToProfile,
      );
      acceptProfile(profile);
      setEnvironmentChanges(emptyActorSecretChanges());
      setNotice(t("voiceSettings.profileCreated", { name: profile.name || name.trim() }));
    } catch (error) {
      setError(
        t("voiceSettings.profileSaveFailed", {
          detail: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setProfileSaving(false);
    }
  };
  return {
    state,
    settings,
    preferences,
    mode,
    selectedProfile,
    compatibleProfiles,
    profileIdentity,
    environmentKeys: state?.environment_keys || [],
    environmentChanges,
    loading,
    saving,
    profileSaving,
    preferencesSaving,
    preferencesError,
    preferencesNotice,
    preferencesDisabled,
    rulesChanged,
    error,
    notice,
    hasChanges,
    settingsLoadFailed,
    editingDisabled,
    environmentRefreshing: loading,
    runtimeChanged,
    configured,
    needsBacklogChoice,
    backlogAction,
    saveDisabled,
    setBacklogAction,
    load,
    refreshEnvironment: load,
    changeMode,
    selectProfile,
    acceptProfile,
    setRuntime,
    setCommand,
    defaultCommand,
    useDefaultCommand,
    setUseDefaultCommand,
    setEnvironmentChanges: (changes: ActorSecretChanges) => {
      setEnvironmentChanges(changes);
      changed();
    },
    updatePreferences,
    savePreferences,
    retryPreferences,
    discardRules,
    discard,
    saveAsProfile,
    save,
  };
}
