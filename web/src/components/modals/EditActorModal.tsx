import { ActorProfile, RuntimeInfo, SupportedRuntime, SUPPORTED_RUNTIMES, RUNTIME_INFO } from "../../types";
import { useTranslation } from "react-i18next";
import { BASIC_MCP_CONFIG_SNIPPET, COPILOT_MCP_CONFIG_SNIPPET, OPENCODE_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { parsePrivateEnvSetText, parsePrivateEnvUnsetText } from "../../utils/privateEnvInput";

type EditMode = "custom" | "profile";

export interface EditActorSavePayload {
  mode: EditMode;
  setVars: Record<string, string>;
  unsetKeys: string[];
  clear: boolean;
  profileId?: string;
  convertToCustom?: boolean;
}

export interface SaveActorProfileResult {
  profileId?: string;
  profileName?: string;
  useNow?: boolean;
}

export const NO_CHANGES_SENTINEL = "CCCC_NO_CHANGES";

export interface EditActorModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  groupId: string;
  actorId: string;
  isRunning: boolean;
  runtimes: RuntimeInfo[];
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  command: string;
  onChangeCommand: (command: string) => void;
  title: string;
  onChangeTitle: (title: string) => void;
  onSave: (payload: EditActorSavePayload) => Promise<void>;
  onSaveAndRestart: (payload: EditActorSavePayload) => Promise<void>;
  linkedProfileId?: string;
  actorProfiles: ActorProfile[];
  actorProfilesBusy: boolean;
  onSaveAsProfile: () => Promise<SaveActorProfileResult | void>;
  inlineNotice?: string;
  onCancel: () => void;
}

type SecretSource = "none" | "actor" | "profile-preview";

/** Runtime-specific placeholder hints for secret environment variables */
const SECRETS_PLACEHOLDER: Record<string, { set: string; unset: string }> = {
  claude: {
    set: 'export ANTHROPIC_API_KEY="...";\nexport ANTHROPIC_BASE_URL="...";',
    unset: "unset ANTHROPIC_API_KEY;\nunset ANTHROPIC_BASE_URL;",
  },
  codex: {
    set: 'export OPENAI_API_KEY="...";\nexport OPENAI_BASE_URL="...";',
    unset: "unset OPENAI_API_KEY;\nunset OPENAI_BASE_URL;",
  },
  gemini: {
    set: 'export GOOGLE_API_KEY="...";',
    unset: "unset GOOGLE_API_KEY;",
  },
};

const DEFAULT_SECRETS_PLACEHOLDER = {
  set: 'export OPENAI_API_KEY="...";\nexport ANTHROPIC_API_KEY="...";\nexport ANTHROPIC_BASE_URL="...";',
  unset: "unset OPENAI_API_KEY;\nunset ANTHROPIC_API_KEY;\nunset ANTHROPIC_BASE_URL;",
};

function commandPreview(command: string[] | undefined): string {
  const cmd = Array.isArray(command) ? command.filter((item) => typeof item === "string" && item.trim()) : [];
  return cmd.join(" ");
}

function modeButtonClass(isDark: boolean, selected: boolean): string {
  return [
    "px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-colors",
    selected
      ? "bg-blue-600 text-white border-blue-600"
      : isDark
        ? "bg-slate-900/60 border-slate-700 text-slate-300"
        : "bg-white border-gray-200 text-gray-700",
  ].join(" ");
}

export function EditActorModal({
  isOpen,
  isDark,
  busy,
  groupId,
  actorId,
  isRunning,
  runtimes,
  runtime,
  onChangeRuntime,
  command,
  onChangeCommand,
  title,
  onChangeTitle,
  onSave,
  onSaveAndRestart,
  linkedProfileId,
  actorProfiles,
  actorProfilesBusy,
  onSaveAsProfile,
  inlineNotice,
  onCancel,
}: EditActorModalProps) {
  const { t } = useTranslation("actors");
  const { modalRef } = useModalA11y(isOpen, onCancel);
  const [secretKeys, setSecretKeys] = useState<string[]>([]);
  const [secretMasks, setSecretMasks] = useState<Record<string, string>>({});
  const [secretsSetText, setSecretsSetText] = useState("");
  const [secretsUnsetText, setSecretsUnsetText] = useState("");
  const [secretsClearAll, setSecretsClearAll] = useState(false);
  const [secretsError, setSecretsError] = useState("");
  const [secretsBusy, setSecretsBusy] = useState(false);
  const [attachProfileId, setAttachProfileId] = useState("");
  const [editMode, setEditMode] = useState<EditMode>("custom");
  const [pendingConvertToCustom, setPendingConvertToCustom] = useState(false);
  const [localNotice, setLocalNotice] = useState("");
  const [secretSource, setSecretSource] = useState<SecretSource>("none");
  const secretFetchSeqRef = useRef(0);
  const modalStateRef = useRef<{
    groupId: string;
    actorId: string;
    linked: boolean;
    editMode: EditMode;
    pendingConvert: boolean;
    profileId: string;
  }>({
    groupId: "",
    actorId: "",
    linked: false,
    editMode: "custom",
    pendingConvert: false,
    profileId: "",
  });

  const linked = Boolean(String(linkedProfileId || "").trim());
  const effectiveLinked = linked && !pendingConvertToCustom;
  const selectedProfile = useMemo(
    () => actorProfiles.find((profile) => String(profile.id || "") === String(attachProfileId || "")),
    [actorProfiles, attachProfileId]
  );
  const selectedProfileName = String(selectedProfile?.name || "").trim();

  const secretsPlaceholder = SECRETS_PLACEHOLDER[runtime] ?? DEFAULT_SECRETS_PLACEHOLDER;

  useEffect(() => {
    modalStateRef.current = {
      groupId,
      actorId,
      linked: effectiveLinked,
      editMode,
      pendingConvert: pendingConvertToCustom,
      profileId: String(linkedProfileId || "").trim(),
    };
  }, [groupId, actorId, effectiveLinked, editMode, pendingConvertToCustom, linkedProfileId]);

  const refreshSecretKeys = async () => {
    if (editMode !== "custom") {
      setSecretKeys([]);
      setSecretMasks({});
      setSecretSource("none");
      return;
    }

    if (linked && pendingConvertToCustom) {
      const profileId = String(linkedProfileId || "").trim();
      if (!profileId) {
        setSecretKeys([]);
        setSecretMasks({});
        setSecretSource("none");
        return;
      }
      const requestSeq = ++secretFetchSeqRef.current;
      const resp = await api.fetchActorProfilePrivateEnvKeys(profileId);
      if (requestSeq !== secretFetchSeqRef.current) return;
      const now = modalStateRef.current;
      if (
        now.groupId !== groupId ||
        now.actorId !== actorId ||
        now.editMode !== "custom" ||
        !now.pendingConvert ||
        now.profileId !== profileId
      ) {
        return;
      }

      if (!resp.ok) {
        setSecretsError(resp.error?.message || t("failedToLoadSecrets"));
        setSecretKeys([]);
        setSecretMasks({});
        setSecretSource("none");
        return;
      }

      const privateKeys = Array.isArray(resp.result?.keys) ? resp.result.keys : [];
      const maskedFromPrivate =
        resp.result?.masked_values && typeof resp.result.masked_values === "object"
          ? resp.result.masked_values
          : {};

      const legacyProfile = actorProfiles.find((profile) => String(profile.id || "") === profileId);
      const legacyEnv = legacyProfile?.env && typeof legacyProfile.env === "object" ? legacyProfile.env : {};
      const legacyKeys = Object.keys(legacyEnv).filter((key) => String(key || "").trim().length > 0);

      const mergedKeys: string[] = [];
      const seen = new Set<string>();
      for (const key of [...privateKeys, ...legacyKeys]) {
        const normalized = String(key || "").trim();
        if (!normalized || seen.has(normalized)) continue;
        seen.add(normalized);
        mergedKeys.push(normalized);
      }

      const mergedMasks: Record<string, string> = {};
      for (const key of mergedKeys) {
        const fromPrivate = String(maskedFromPrivate[key] || "").trim();
        mergedMasks[key] = fromPrivate || "******";
      }

      setSecretsError("");
      setSecretKeys(mergedKeys);
      setSecretMasks(mergedMasks);
      setSecretSource("profile-preview");
      return;
    }

    if (effectiveLinked) {
      setSecretKeys([]);
      setSecretMasks({});
      setSecretSource("none");
      return;
    }

    if (!groupId || !actorId) return;
    const requestForGroupId = groupId;
    const requestForActorId = actorId;
    const requestSeq = ++secretFetchSeqRef.current;
    const resp = await api.fetchActorPrivateEnvKeys(requestForGroupId, requestForActorId);
    if (requestSeq !== secretFetchSeqRef.current) return;
    const now = modalStateRef.current;
    if (
      now.groupId !== requestForGroupId ||
      now.actorId !== requestForActorId ||
      now.linked ||
      now.editMode !== "custom"
    ) {
      return;
    }
    if (!resp.ok) {
      const code = String(resp.error?.code || "").trim();
      if (code === "actor_profile_linked_readonly") {
        // Treat as state transition, not a user-facing error.
        setSecretsError("");
        setSecretKeys([]);
        setSecretMasks({});
        setSecretSource("none");
        return;
      }
      setSecretsError(resp.error?.message || t("failedToLoadSecrets"));
      setSecretKeys([]);
      setSecretMasks({});
      setSecretSource("none");
      return;
    }
    const keys = Array.isArray(resp.result?.keys) ? resp.result.keys : [];
    const masked = resp.result?.masked_values && typeof resp.result.masked_values === "object" ? resp.result.masked_values : {};
    setSecretKeys(keys);
    setSecretMasks(masked);
    setSecretSource("actor");
  };

  useEffect(() => {
    if (!isOpen) return;
    secretFetchSeqRef.current += 1;
    const hasLinked = Boolean(String(linkedProfileId || "").trim());
    setEditMode(hasLinked ? "profile" : "custom");
    setPendingConvertToCustom(false);
    setAttachProfileId(hasLinked ? String(linkedProfileId || "") : "");
    setLocalNotice("");
    setSecretsError("");
    setSecretMasks({});
    setSecretsSetText("");
    setSecretsUnsetText("");
    setSecretsClearAll(false);
    setSecretKeys([]);
    setSecretSource("none");
    if (!hasLinked) void refreshSecretKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupId, actorId, isOpen, linkedProfileId]);

  useEffect(() => {
    if (!isOpen) return;
    const linkedId = String(linkedProfileId || "").trim();
    if (!linkedId || actorProfilesBusy) return;
    const exists = actorProfiles.some((profile) => String(profile.id || "").trim() === linkedId);
    if (exists) return;
    // Defensive fallback: when profile was deleted/detached and actor list is briefly stale,
    // prefer Custom tab to avoid opening in an invalid "Use Profile" state.
    setEditMode("custom");
    setPendingConvertToCustom(false);
    setAttachProfileId("");
  }, [isOpen, linkedProfileId, actorProfilesBusy, actorProfiles]);

  useEffect(() => {
    if (!isOpen) return;
    if (editMode === "profile") {
      secretFetchSeqRef.current += 1;
      setSecretKeys([]);
      setSecretMasks({});
      setSecretSource("none");
      return;
    }
    if (!effectiveLinked) void refreshSecretKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editMode, effectiveLinked]);

  if (!isOpen) return null;

  const rtInfo = runtimes.find((r) => r.name === runtime);
  const available = rtInfo?.available ?? false;
  const defaultCommand = rtInfo?.recommended_command || "";
  const requireCommand = !effectiveLinked && editMode === "custom" && (runtime === "custom" || !available);

  const convertToCustomDraft = () => {
    if (!linked || busy === "actor-update") return;
    setSecretsError("");
    setLocalNotice("");
    setPendingConvertToCustom(true);
    setEditMode("custom");
  };

  const saveAsProfile = async () => {
    setSecretsError("");
    setLocalNotice("");
    try {
      const result = await onSaveAsProfile();
      const profileId = String(result?.profileId || "").trim();
      if (profileId && result?.useNow) {
        setPendingConvertToCustom(false);
        setEditMode("profile");
        setAttachProfileId(profileId);
        setLocalNotice(
          t("profileSelectedPendingSave", {
            name: String(result.profileName || profileId),
          })
        );
      }
    } catch (e) {
      setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
    }
  };

  const submit = async (restart: boolean) => {
    if (!groupId || !actorId) return;
    if (busy === "actor-update") return;
    const callback = restart ? onSaveAndRestart : onSave;

    if (editMode === "profile") {
      const profileId = String(attachProfileId || "").trim();
      if (!profileId) {
        setSecretsError(t("profileRequired"));
        return;
      }
      setSecretsError("");
      setLocalNotice("");
      setSecretsBusy(true);
      try {
        await callback({
          mode: "profile",
          setVars: {},
          unsetKeys: [],
          clear: false,
          profileId,
        });
      } catch (e) {
        const msg = e instanceof Error ? e.message : "";
        if (msg === NO_CHANGES_SENTINEL) {
          setSecretsError("");
          setLocalNotice(t("nothingToSave"));
        } else {
          setLocalNotice("");
          setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
        }
        return;
      } finally {
        setSecretsBusy(false);
      }
      return;
    }

    if (effectiveLinked) {
      setSecretsError(t("profileControlsRuntimeFields"));
      return;
    }

    setSecretsError("");
    const setParsed = parsePrivateEnvSetText(secretsSetText);
    if (!setParsed.ok) {
      setSecretsError(setParsed.error);
      return;
    }
    const unsetParsed = parsePrivateEnvUnsetText(secretsUnsetText);
    if (!unsetParsed.ok) {
      setSecretsError(unsetParsed.error);
      return;
    }

    setSecretsBusy(true);
    try {
      await callback({
        mode: "custom",
        setVars: setParsed.setVars,
        unsetKeys: unsetParsed.unsetKeys,
        clear: secretsClearAll,
        convertToCustom: linked && pendingConvertToCustom,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "";
      if (msg === NO_CHANGES_SENTINEL) {
        setSecretsError("");
        setLocalNotice(t("nothingToSave"));
      } else {
        setLocalNotice("");
        setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
      }
      return;
    } finally {
      setSecretsBusy(false);
    }
  };

  return (
    <div
      className={`fixed inset-0 flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/60" : "bg-black/40"}`}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="edit-actor-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-w-md sm:mt-16 border shadow-2xl animate-scale-in flex flex-col sm:max-h-[calc(100vh-8rem)] rounded-none sm:rounded-2xl ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="edit-actor-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            {t("editAgent", { actorId })}
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("changeSettings")}</div>
        </div>
        <div className="p-6 space-y-5 overflow-y-auto flex-1 min-h-0">
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("displayName")}</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={title}
              onChange={(e) => onChangeTitle(e.target.value)}
              placeholder={actorId}
            />
            <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("leaveEmptyForId")}</div>
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("creationMode")}</label>
            <div className="grid grid-cols-2 gap-2">
              <button type="button" className={modeButtonClass(isDark, editMode === "custom")} onClick={() => setEditMode("custom")}>
                {t("customAgent")}
              </button>
              <button
                type="button"
                className={modeButtonClass(isDark, editMode === "profile")}
                onClick={() => {
                  setEditMode("profile");
                  setPendingConvertToCustom(false);
                  setLocalNotice("");
                }}
              >
                {t("fromActorProfile")}
              </button>
            </div>
          </div>

          {editMode === "profile" ? (
            <>
              <div>
                <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("actorProfile")}</label>
                <select
                  className={`w-full rounded-xl border px-3 py-2 text-sm min-h-[40px] ${
                    isDark ? "bg-slate-900/80 border-slate-600/50 text-white" : "bg-white border-gray-300 text-gray-900"
                  }`}
                  value={attachProfileId}
                  onChange={(e) => setAttachProfileId(e.target.value)}
                  disabled={actorProfilesBusy || busy === "actor-update"}
                >
                  <option value="">{actorProfilesBusy ? t("loadingProfiles") : t("selectActorProfile")}</option>
                  {actorProfiles.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.name || profile.id}
                    </option>
                  ))}
                </select>
              </div>

              {selectedProfile ? (
                <div className={`rounded-xl border px-3 py-2 text-xs ${isDark ? "border-slate-700 bg-slate-900/60 text-slate-300" : "border-gray-200 bg-gray-50 text-gray-700"}`}>
                  <div className="font-medium">{selectedProfile.name || selectedProfile.id}</div>
                  <div className="mt-1">
                    {RUNTIME_INFO[String(selectedProfile.runtime) as SupportedRuntime]?.label || selectedProfile.runtime}
                    {" · "}
                    {String(selectedProfile.runner || "pty").toUpperCase()}
                  </div>
                  {commandPreview(selectedProfile.command) ? <div className="mt-1 font-mono break-all">{commandPreview(selectedProfile.command)}</div> : null}
                </div>
              ) : null}
            </>
          ) : effectiveLinked ? (
            <div className={`rounded-xl border px-3 py-2 ${isDark ? "border-sky-500/30 bg-sky-500/10 text-sky-100" : "border-sky-200 bg-sky-50 text-sky-800"}`}>
              <div className="text-sm font-medium">
                {selectedProfileName ? t("managedByProfileName", { name: selectedProfileName }) : t("managedByProfile")}
              </div>
              <div className={`text-xs mt-1 ${isDark ? "text-sky-200/80" : "text-sky-700"}`}>{t("managedByProfileCustomHint")}</div>
              <button
                type="button"
                className={`mt-2 px-3 py-2 rounded-lg text-sm font-medium ${
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white border border-gray-200 hover:bg-gray-50 text-gray-700"
                }`}
                onClick={convertToCustomDraft}
                disabled={busy === "actor-update"}
              >
                {t("convertToCustom")}
              </button>
            </div>
          ) : (
            <>
              <div>
                <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("runtime")}</label>
                <select
                  className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                    isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                  }`}
                  value={runtime}
                  onChange={(e) => {
                    const next = e.target.value as SupportedRuntime;
                    onChangeRuntime(next);
                    const nextInfo = runtimes.find((r) => r.name === next);
                    const nextDefault = String(nextInfo?.recommended_command || "").trim();
                    onChangeCommand(nextDefault);
                  }}
                >
                  {SUPPORTED_RUNTIMES.map((rt) => {
                    const info = RUNTIME_INFO[rt];
                    const rtInfo = runtimes.find((r) => r.name === rt);
                    const runtimeAvailable = rtInfo?.available ?? false;
                    const selectable = runtimeAvailable || rt === "custom";
                    return (
                      <option key={rt} value={rt} disabled={!selectable}>
                        {info?.label || rt}
                        {!runtimeAvailable && rt !== "custom" ? ` ${t("notInstalled")}` : ""}
                      </option>
                    );
                  })}
                </select>

                {(runtime === "cursor" || runtime === "kilocode" || runtime === "opencode" || runtime === "copilot" || runtime === "custom") && (
                  <div
                    className={`mt-2 rounded-xl border px-3 py-2 text-[11px] ${
                      isDark ? "border-amber-500/30 bg-amber-500/10 text-amber-200" : "border-amber-200 bg-amber-50 text-amber-800"
                    }`}
                  >
                    <div className="font-medium">{t("manualMcpRequired")}</div>
                    {runtime === "custom" ? (
                      <>
                        <div className="mt-1">
                          {t("configureMcpStdio")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc</code> {t("thatRuns")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>cccc mcp</code>.
                        </div>
                      </>
                    ) : runtime === "cursor" ? (
                      <>
                        <div className="mt-1">
                          {t("createEditFile")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.cursor/mcp.json</code> (or <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.cursor/mcp.json</code> {t("orInProject")})
                        </div>
                        <div className="mt-1">{t("addMcpConfig")}</div>
                      </>
                    ) : runtime === "kilocode" ? (
                      <>
                        <div className="mt-1">
                          {t("createEditFile")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>.kilocode/mcp.json</code> {t("inProjectRoot")}
                        </div>
                        <div className="mt-1">{t("addMcpConfig")}</div>
                      </>
                    ) : runtime === "opencode" ? (
                      <>
                        <div className="mt-1">
                          {t("createEditFile")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.config/opencode/opencode.json</code>
                        </div>
                        <div className="mt-1">{t("addMcpConfig")}</div>
                      </>
                    ) : (
                      <>
                        <div className="mt-1">
                          {t("createEditFile")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>~/.copilot/mcp-config.json</code>
                        </div>
                        <div className="mt-1">
                          {t("addMcpConfigOrFlag")} <code className={`px-1 rounded ${isDark ? "bg-amber-900/30" : "bg-amber-100"}`}>--additional-mcp-config</code>):
                        </div>
                      </>
                    )}
                    {runtime !== "custom" ? (
                      <pre
                        className={`mt-1.5 p-2 rounded overflow-x-auto whitespace-pre ${
                          isDark ? "bg-amber-900/20 text-amber-100" : "bg-amber-50 text-amber-900"
                        }`}
                      >
                        <code>{runtime === "opencode" ? OPENCODE_MCP_CONFIG_SNIPPET : runtime === "copilot" ? COPILOT_MCP_CONFIG_SNIPPET : BASIC_MCP_CONFIG_SNIPPET}</code>
                      </pre>
                    ) : null}
                    <div className={`mt-1 text-[10px] ${isDark ? "text-amber-200/80" : "text-amber-800/80"}`}>{t("restartAfterConfig")}</div>
                  </div>
                )}
              </div>

              <div>
                <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("command")}</label>
                <input
                  className={`w-full rounded-xl border px-4 py-2.5 text-sm font-mono min-h-[44px] transition-colors ${
                    isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                  }`}
                  value={command}
                  onChange={(e) => onChangeCommand(e.target.value)}
                  placeholder={defaultCommand || t("enterCommand")}
                />
                {isRunning ? <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("runtimeChangesNote")}</div> : null}
                {defaultCommand.trim() ? (
                  <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                    {t("default")} <code className={`px-1 rounded ${isDark ? "bg-slate-800" : "bg-gray-100"}`}>{defaultCommand}</code>
                  </div>
                ) : null}
              </div>

              <div>
                <div className="flex items-center justify-between gap-3">
                  <label className={`block text-xs font-medium ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("secretsWriteOnly")}</label>
                  <button
                    className={`text-xs px-2 py-1 rounded-lg border transition-colors ${
                      isDark ? "border-slate-600/50 text-slate-300 hover:bg-slate-800" : "border-gray-200 text-gray-700 hover:bg-gray-50"
                    }`}
                    onClick={() => void refreshSecretKeys()}
                    disabled={secretsBusy}
                    title={t("refreshConfiguredKeys")}
                  >
                    {t("refresh")}
                  </button>
                </div>
                <div className={`text-[10px] mt-1.5 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {t("secretsStoredLocallyEdit").replace(/<1>|<\/1>/g, "")} {secretKeys.length ? (
                    <>
                      {t("configuredKeys")}
                      <div className="mt-1.5 flex flex-wrap gap-1.5">
                        {secretKeys.map((key) => {
                          const masked = String(secretMasks[key] || "******");
                          return (
                            <span
                              key={key}
                              className={`inline-flex items-center rounded-md border px-2 py-0.5 font-mono text-[10px] ${isDark ? "border-slate-600/50 text-slate-200 bg-slate-900/70" : "border-gray-300 text-gray-700 bg-gray-50"}`}
                              title={`${t("maskedPreview")}: ${masked}`}
                            >
                              {key}
                            </span>
                          );
                        })}
                      </div>
                      <div className="mt-1">{t("hoverToPreviewMasked")}</div>
                      {secretSource === "profile-preview" ? (
                        <div className="mt-1">{t("pendingConvertSecretsPreview")}</div>
                      ) : null}
                    </>
                  ) : (
                    <>{t("noKeysConfigured")}</>
                  )}
                </div>
                <div className={`text-[10px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("secretsAppliedNote").replace(/<1>|<\/1>/g, "")}</div>

                <label className={`block text-[11px] font-medium mt-3 mb-1.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>{t("setUpdate")}</label>
                <textarea
                  className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[96px] transition-colors ${
                    isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                  }`}
                  value={secretsSetText}
                  onChange={(e) => setSecretsSetText(e.target.value)}
                  placeholder={secretsPlaceholder.set}
                />

                <label className={`block text-[11px] font-medium mt-3 mb-1.5 ${isDark ? "text-slate-500" : "text-gray-600"}`}>{t("unset")}</label>
                <textarea
                  className={`w-full rounded-xl border px-3 py-2 text-sm font-mono min-h-[72px] transition-colors ${
                    isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                  }`}
                  value={secretsUnsetText}
                  onChange={(e) => setSecretsUnsetText(e.target.value)}
                  placeholder={secretsPlaceholder.unset}
                />

                <label className={`flex items-center gap-2 text-[11px] font-medium mt-3 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                  <input
                    type="checkbox"
                    checked={secretsClearAll}
                    onChange={(e) => setSecretsClearAll(e.target.checked)}
                    disabled={secretsBusy || busy === "actor-update"}
                  />
                  {t("clearAllKeys")}
                </label>
              </div>

              <div className="flex flex-wrap gap-3 pt-2">
                <button
                  type="button"
                  className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                    isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                  }`}
                  onClick={() => void saveAsProfile()}
                  disabled={busy === "actor-profile-save" || busy === "actor-update"}
                >
                  {busy === "actor-profile-save" ? t("savingProfile") : t("addToActorProfiles")}
                </button>
              </div>
            </>
          )}

          {secretsError ? (
            <div
              className={`mt-2 rounded-xl border px-3 py-2 text-xs ${
                isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
              }`}
              role="alert"
            >
              {secretsError}
            </div>
          ) : null}

          {String(localNotice || inlineNotice || "").trim() ? (
            <div
              className={`mt-2 rounded-xl border px-3 py-2 text-xs ${
                isDark ? "border-sky-500/30 bg-sky-500/10 text-sky-200" : "border-sky-200 bg-sky-50 text-sky-800"
              }`}
              role="status"
            >
              {String(localNotice || inlineNotice || "").trim()}
            </div>
          ) : null}

          <div className="flex flex-col sm:flex-row gap-3 pt-2">
            <button
              className="flex-1 rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={() => void submit(false)}
              disabled={
                busy === "actor-update" ||
                secretsBusy ||
                (editMode === "custom" && effectiveLinked) ||
                (editMode === "custom" && requireCommand && !command.trim()) ||
                (editMode === "profile" && !String(attachProfileId || "").trim())
              }
            >
              {t("common:save")}
            </button>
            <button
              className="flex-1 rounded-xl bg-sky-700 hover:bg-sky-600 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={() => void submit(true)}
              disabled={
                busy === "actor-update" ||
                secretsBusy ||
                (editMode === "custom" && effectiveLinked) ||
                (editMode === "custom" && requireCommand && !command.trim()) ||
                (editMode === "profile" && !String(attachProfileId || "").trim())
              }
            >
              {t("saveAndRestart")}
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancel}
            >
              {t("common:cancel")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
