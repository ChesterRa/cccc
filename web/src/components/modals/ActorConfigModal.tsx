import { LocalSessionsDialog } from "../../features/localSessions/LocalSessionsDialog";
import { RuntimeCategoryPicker } from "../../features/runtimeSwitch/RuntimeCategoryPicker";
import {
  actorRuntimeCategory,
  type RuntimeCategory,
} from "../../features/runtimeSwitch/runtimeCategory";
import {
  RuntimeSwitchConfirm,
  RuntimeSwitchControls,
} from "../../features/runtimeSwitch/RuntimeSwitchControls";
import { RuntimeSwitchPendingNotice } from "../../features/runtimeSwitch/RuntimeSwitchPendingNotice";
import { joinRuntimeSentences } from "../../features/runtimeSwitch/runtimeSwitchOutcome";
import {
  hasPendingActorSwitch,
  openWebSetup,
} from "../../features/runtimeSwitch/runtimeSwitchState";
import { supportsAcpMode } from "../../types";
import { GrokActorSetup } from "../webModel/GrokActorSetup";
import { isWebModelRuntime } from "../../types";
import { actorGenerationIdentity } from "../../utils/actorIdentity";
import { type Actor, ActorProfile, GroupDoc, RuntimeInfo, SupportedRuntime } from "../../types";
import { useTranslation } from "react-i18next";
import { BASIC_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { formatCapabilityIdInput, parseCapabilityIdInput } from "../../utils/capabilityAutoload";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { formatRuntimeCommand } from "./runtimeProfileControlsModel";
import { CapabilityPicker } from "../CapabilityPicker";
import { RolePresetPicker } from "../RolePresetPicker";
import { ActorAvatarField } from "../ActorAvatarField";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";
import { Textarea } from "../ui/textarea";
import { ActorWorkingDirectory } from "./ActorWorkingDirectory";
import { ActorConfigTabs } from "./ActorConfigTabs";
import { ActorSecretManager } from "./ActorSecretManager";
import { DeepSeekCommandDetails, DeepSeekConnectionFields } from "./DeepSeekConnectionFields";
import { deepSeekSettingsError, normalizeDeepSeekSettings } from "./deepseekSettingsModel";
import { parsePrivateEnvSetText } from "../../utils/privateEnvInput";
import {
  OpenCodeManagedModelHint,
  RuntimeCommandControl,
  RuntimeConfigurationModePicker,
  RuntimeProfilePicker,
} from "./RuntimeProfileControls";
import {
  buildActorSecretSaveChanges,
  applyActorSecretSaveChanges,
  emptyActorSecretChanges,
  normalizeLoadedActorSecretKeys,
  type ActorSecretChanges,
  type ActorSecretSaveChanges,
} from "./actorSecretManagerModel";
import { WebModelActorSetup } from "../webModel/WebModelActorSetup";
import { DotsActorPanel } from "../webModel/DotsActorPanel";
import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import { SUPPORTED_RUNTIMES } from "../../types";
import { isGrokBotUrl } from "../../utils/webModelTargetDraft";
import { ModalFrame } from "./ModalFrame";
import { useChatObscuringView } from "../../hooks/useChatObscuringView";

// Centered with equal breathing room above and below: the frame already pads
// the viewport by 1rem on each side, so this leaves 3rem clear top and bottom.
const ACTOR_MODAL_PANEL_CLASS =
  "w-full h-full sm:h-auto sm:w-[min(100vw-2rem,72rem)] sm:max-w-[72rem] sm:max-h-[calc(100dvh-6rem)]";

import { AcpRuntimeMode } from "./AcpRuntimeMode";
import { RuntimeSelector } from "./RuntimeSelector";
import type { RuntimeMode } from "../../types";

type ConfigMode = "custom" | "profile";
type AdvancedTabId = "connection" | "environment" | "capabilities" | "profile";

export interface EditActorSavePayload {
  mode: ConfigMode;
  runtimeMode?: RuntimeMode;
  setVars: Record<string, string>;
  unsetKeys: string[];
  clear: boolean;
  capabilityAutoload: string[];
  defaultScopeKey?: string;
  profileId?: string;
  convertToCustom?: boolean;
  grokBotUrl?: string;
  // A later notes/binding/restart failure must not replay committed secret edits.
  onSecretsSaved?: (keys: string[]) => void;
  runtimeSwitch?: { start: boolean };
}

export interface SaveActorProfileResult {
  profileId?: string;
  profileName?: string;
  useNow?: boolean;
}

interface ActorConfigBaseProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  runtimes: RuntimeInfo[];
  actorProfiles: ActorProfile[];
  actorProfilesBusy: boolean;
  groupId?: string;
  scopes?: GroupDoc["scopes"];
  activeScopeKey?: string;
  onScopeAttached?: (groupId: string, scope: api.AttachScopeResult["scope"]) => void;
  onRequestActorProfiles?: () => Promise<void> | void;
  onSaveAsProfile: (
    secrets?: ActorSecretSaveChanges,
    runtimeMode?: RuntimeMode,
    options?: { savedActor?: boolean },
  ) => Promise<SaveActorProfileResult | void> | void;
  onCancel: () => void;
}

export interface EditActorConfigProps extends ActorConfigBaseProps {
  mode: "edit";
  initialSection?: "chatgpt" | null;
  groupId: string;
  actorId: string;
  groupRole?: "foreman" | "peer" | string;
  avatarUrl?: string | null;
  hasCustomAvatar?: boolean;
  isRunning: boolean;
  savedRuntime: string;
  savedActor?: Actor;
  savedActorNotes?: string;
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  command: string;
  onChangeCommand: (command: string) => void;
  title: string;
  onChangeTitle: (title: string) => void;
  actorNotes: string;
  onChangeActorNotes: (value: string) => void;
  actorNotesBusy?: boolean;
  actorNotesError?: boolean;
  onRetryActorNotes?: () => void;
  capabilityAutoloadText: string;
  onChangeCapabilityAutoloadText: (value: string) => void;
  onSave: (payload: EditActorSavePayload) => Promise<void>;
  onSaveAndRestart: (payload: EditActorSavePayload) => Promise<void>;
  onSaveBasics?: () => Promise<void>;
  linkedProfileId?: string;
  linkedProfileScope?: "global" | "user";
  linkedProfileOwner?: string;
  onAvatarChanged?: (updated: Actor, previous?: Actor) => Promise<boolean | void>;
  inlineNotice?: string;
}

export interface CreateActorConfigProps extends ActorConfigBaseProps {
  mode: "create";
  hasForeman: boolean;
  suggestedActorId: string;
  actorId: string;
  onChangeActorId: (id: string) => void;
  role: "peer" | "foreman";
  onChangeRole: (role: "peer" | "foreman") => void;
  useProfile: boolean;
  onChangeUseProfile: (value: boolean) => void;
  profileId: string;
  onChangeProfileId: (id: string) => void;
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  command: string;
  onChangeCommand: (command: string) => void;
  useDefaultCommand: boolean;
  onChangeUseDefaultCommand: (value: boolean) => void;
  secretsSetText: string;
  onChangeSecretsSetText: (value: string) => void;
  capabilityAutoloadText: string;
  onChangeCapabilityAutoloadText: (value: string) => void;
  actorNotes: string;
  onChangeActorNotes: (value: string) => void;
  error: string;
  onChangeError: (message: string) => void;
  canSubmit: boolean;
  submitDisabledReason: string;
  onCreate: (
    avatarFile?: File | null,
    runtimeMode?: RuntimeMode,
    defaultScopeKey?: string,
    environment?: ActorSecretSaveChanges,
  ) => Promise<boolean> | boolean;
  onSelectGroup?: (groupId: string) => void;
}

export type ActorConfigModalProps = EditActorConfigProps | CreateActorConfigProps;

/** Runtime-specific placeholder hints for secret environment variables */
const SECRETS_PLACEHOLDER: Record<string, { set: string; unset: string }> = {
  claude: {
    set: 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."',
    unset: "ANTHROPIC_AUTH_TOKEN\nANTHROPIC_BASE_URL",
  },
  cline: {
    set: "# Configure Cline providers with `cline auth` or Cline configuration.",
    unset: "",
  },
  codex: {
    set: "# Configure OpenAI-compatible Codex providers with Codex config or command -c overrides.",
    unset: "",
  },
  copilot: {
    set: "# Configure GitHub Copilot CLI auth with `copilot` before starting this actor.",
    unset: "",
  },
  cursor: {
    set: "# Configure Cursor CLI auth with `cursor-agent login` before starting this actor.",
    unset: "",
  },
  devin: { set: "# Configure Devin auth and providers through Devin CLI setup.", unset: "" },
  kiro: {
    set: "# Configure Kiro auth with kiro-cli login and providers through Kiro CLI setup.",
    unset: "",
  },
  kilo: {
    set: "# Configure Kilo Code CLI auth with `kilo` and `/connect` before starting this actor.",
    unset: "",
  },
  antigravity: {
    set: "# Configure Antigravity CLI auth with `agy` before starting this actor.",
    unset: "",
  },
  grok: {
    set: "# Configure Grok providers and auth through Grok config or environment variables.",
    unset: "",
  },
  hermes: {
    set: "# Configure Hermes providers, OAuth, and tools in your Hermes profile.",
    unset: "",
  },
  opencode: {
    set: "# Configure OpenCode providers and auth through OpenCode config or environment variables.",
    unset: "",
  },
};

const DEFAULT_SECRETS_PLACEHOLDER = {
  set: 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."',
  unset: "ANTHROPIC_AUTH_TOKEN\nANTHROPIC_BASE_URL",
};

function modeButtonClass(selected: boolean): string {
  return [
    "min-w-0 whitespace-normal px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-all ease-spring duration-300",
    selected
      ? "border-[var(--color-text-primary)] bg-[var(--color-text-primary)] text-[var(--color-bg-primary)] hover:bg-[var(--color-text-primary)] hover:text-[var(--color-bg-primary)] hover:opacity-90"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
  ].join(" ");
}

function normalizeGroupRole(role: unknown): "foreman" | "peer" {
  return String(role || "")
    .trim()
    .toLowerCase() === "foreman"
    ? "foreman"
    : "peer";
}

function groupRoleBadgeClass(role: "foreman" | "peer"): string {
  return [
    "inline-flex items-center rounded-md border px-1.5 py-0.5 text-[10px] font-medium",
    role === "foreman"
      ? "border-amber-500/25 bg-amber-500/12 text-amber-700 dark:text-amber-300"
      : "border-slate-400/25 bg-slate-500/10 text-slate-600 dark:border-slate-400/20 dark:bg-slate-400/10 dark:text-slate-300",
  ].join(" ");
}

function CreateActorConfigModal({
  isOpen,
  isDark,
  busy,
  groupId,
  scopes = [],
  activeScopeKey,
  onScopeAttached,
  hasForeman,
  runtimes,
  suggestedActorId,
  actorId,
  onChangeActorId,
  role,
  onChangeRole,
  useProfile,
  onChangeUseProfile,
  profileId,
  onChangeProfileId,
  actorProfiles,
  actorProfilesBusy,
  onRequestActorProfiles,
  runtime,
  onChangeRuntime,
  command,
  onChangeCommand,
  useDefaultCommand,
  onChangeUseDefaultCommand,
  secretsSetText,
  onChangeSecretsSetText,
  capabilityAutoloadText,
  onChangeCapabilityAutoloadText,
  actorNotes,
  onChangeActorNotes,
  error,
  onChangeError,
  canSubmit,
  submitDisabledReason,
  onCreate,
  onSaveAsProfile,
  onCancel,
  onSelectGroup,
}: CreateActorConfigProps) {
  const { t } = useTranslation("actors");
  const [localDialogOpen, setLocalDialogOpen] = useState(false);
  const [createCategory, setCreateCategory] = useState<RuntimeCategory>(
    isWebModelRuntime(runtime) ? "web" : "managed",
  );
  const canJoinNative = useUIStore((state) => state.isWebAdmin === true && !state.webReadOnly);
  const { modalRef } = useModalA11y(isOpen && !localDialogOpen, onCancel);
  useChatObscuringView(isOpen);
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode>("default");
  const createDrafts = useRef<
    Partial<
      Record<
        "managed" | "web",
        {
          runtime: SupportedRuntime;
          command: string;
          useDefaultCommand: boolean;
          runtimeMode: RuntimeMode;
          useProfile: boolean;
          profileId: string;
        }
      >
    >
  >({});
  const [defaultScopeKey, setDefaultScopeKey] = useState("");
  const [directoryBusy, setDirectoryBusy] = useState(false);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [capabilitiesPrimed, setCapabilitiesPrimed] = useState(false);
  const [advancedTab, setAdvancedTab] = useState<AdvancedTabId>("environment");
  const [deepSeekChanges, setDeepSeekChanges] =
    useState<ActorSecretChanges>(emptyActorSecretChanges);
  const avatarPreviewUrl = useMemo(
    () => (avatarFile ? URL.createObjectURL(avatarFile) : null),
    [avatarFile],
  );

  useEffect(() => {
    if (!isOpen) {
      setRuntimeMode("default");
      setLocalDialogOpen(false);
      setDeepSeekChanges(emptyActorSecretChanges());
    }
  }, [isOpen]);

  useEffect(() => {
    createDrafts.current = {};
    if (isOpen) setCreateCategory(isWebModelRuntime(runtime) ? "web" : "managed");
    // The selected category is a draft during this modal visit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  useEffect(() => {
    setDefaultScopeKey("");
  }, [groupId, isOpen]);

  useEffect(() => {
    return () => {
      if (avatarPreviewUrl) URL.revokeObjectURL(avatarPreviewUrl);
    };
  }, [avatarPreviewUrl]);

  const selectableActorProfiles = useMemo(
    () =>
      actorProfiles.filter(
        (profile) => isWebModelRuntime(profile.runtime) === (createCategory === "web"),
      ),
    [actorProfiles, createCategory],
  );

  if (!isOpen) return null;

  const selectedProfile = selectableActorProfiles.find(
    (item) => actorProfileIdentityKey(item) === String(profileId || "").trim(),
  );
  const attachedScopes = scopes.filter((scope) => scope.scope_key && scope.url);
  const selectedScope = attachedScopes.find(
    (scope) => scope.scope_key === (defaultScopeKey || activeScopeKey),
  );
  const scopeUnavailable = Boolean(defaultScopeKey && !selectedScope);
  const selectedProfileRuntime = String(selectedProfile?.runtime || "").trim() as SupportedRuntime;
  const runtimeInfo = runtimes.find((r) => r.name === runtime);
  const defaultCommand = runtimeInfo?.recommended_command || "";
  const previewRuntime = useProfile ? selectedProfileRuntime || null : runtime;
  const previewTitle = String(actorId || "").trim() || suggestedActorId;
  const showRuntimeSetup = !useProfile && runtime === "custom";
  const webModelSetupIsActorBound = !useProfile && isWebModelRuntime(runtime);
  const secretsPlaceholder = (SECRETS_PLACEHOLDER[runtime] ?? DEFAULT_SECRETS_PLACEHOLDER).set;
  const parsedSecrets = parsePrivateEnvSetText(secretsSetText);
  const initialSecrets = parsedSecrets.ok ? parsedSecrets.setVars : {};
  const deepSeekSaveChanges = buildActorSecretSaveChanges(
    normalizeDeepSeekSettings(deepSeekChanges),
  );
  const deepSeekError =
    runtime === "deepseek" && !useProfile
      ? deepSeekSettingsError(applyActorSecretSaveChanges(initialSecrets, deepSeekSaveChanges))
      : "";
  const sectionCardClass = "min-w-0 rounded-2xl p-4 sm:p-5 glass-panel";
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const createAdvancedTabIds: AdvancedTabId[] = [
    ...(showRuntimeSetup ? ["connection" as const] : []),
    ...(!useProfile && !isWebModelRuntime(runtime) ? ["environment" as const] : []),
    ...(previewRuntime ? ["capabilities" as const] : []),
    ...(!useProfile ? ["profile" as const] : []),
  ];
  const activeAdvancedTab = createAdvancedTabIds.includes(advancedTab)
    ? advancedTab
    : createAdvancedTabIds[0];
  const selectAdvancedTab = (id: string) => {
    const next = id as AdvancedTabId;
    setAdvancedTab(next);
    if (next === "capabilities") setCapabilitiesPrimed(true);
  };

  const handleSubmit = async () => {
    if (scopeUnavailable || directoryBusy) return;
    if (deepSeekError) {
      onChangeError(t(`deepseek.${deepSeekError}`));
      return;
    }
    if (String(secretsSetText || "").trim() && (useProfile || isWebModelRuntime(runtime))) {
      onChangeError(t("secretDraftNotApplied"));
      return;
    }
    if (useProfile && !selectedProfile) {
      onChangeError(t("profileRequired"));
      return;
    }
    try {
      const ok = await Promise.resolve(
        runtime === "deepseek" && !useProfile
          ? onCreate(avatarFile, "default", defaultScopeKey, deepSeekSaveChanges)
          : onCreate(
              avatarFile,
              supportsAcpMode(runtime) ? runtimeMode : "default",
              defaultScopeKey,
            ),
      );
      if (ok) setAvatarFile(null);
    } catch (e) {
      onChangeError(e instanceof Error ? e.message : t("failedToAddAgent"));
    }
  };

  const handleCancel = () => {
    setAvatarFile(null);
    onCancel();
  };

  return (
    <ModalFrame
      isOpen={isOpen && !localDialogOpen}
      isDark={isDark}
      onClose={handleCancel}
      titleId="actor-config-create-title"
      title={
        <div>
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("addAiAgent")}
          </div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName={ACTOR_MODAL_PANEL_CLASS}
      modalRef={modalRef}
      footerActions={
        <>
          {error ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-rose-500/20 bg-rose-500/5 text-rose-600 dark:text-rose-400"
              role="alert"
              data-status-tone="error"
            >
              <div className="flex items-start justify-between gap-3">
                <span>{error}</span>
                <button
                  type="button"
                  className="text-rose-600 dark:text-rose-400 hover:opacity-80"
                  onClick={() => onChangeError("")}
                  aria-label={t("common:close")}
                >
                  x
                </button>
              </div>
            </div>
          ) : null}

          <div className="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
            <Button
              type="button"
              variant="secondary"
              className="w-full sm:w-auto"
              onClick={handleCancel}
            >
              {t("common:cancel")}
            </Button>
            {createCategory !== "native" ? (
              <div className="w-full sm:w-auto sm:min-w-[14rem]">
                <Button
                  type="button"
                  className="w-full font-semibold"
                  onClick={() => void handleSubmit()}
                  disabled={
                    !canSubmit || scopeUnavailable || directoryBusy || Boolean(deepSeekError)
                  }
                >
                  {busy === "actor-add"
                    ? t("adding")
                    : useProfile
                      ? t("createFromProfile")
                      : t("addAgent")}
                </Button>
                {scopeUnavailable || submitDisabledReason ? (
                  <div
                    className="text-[10px] text-amber-700 dark:text-amber-400 mt-1.5"
                    data-status-tone="warning"
                  >
                    {scopeUnavailable ? t("workingDirectoryUnavailable") : submitDisabledReason}
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        </>
      }
    >
      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-hide bg-[var(--color-bg-primary)] p-4 sm:p-6">
        <div className="mx-auto w-full max-w-6xl space-y-4">
          <Surface className={sectionCardClass}>
            <RuntimeCategoryPicker
              value={createCategory}
              current="managed"
              canSwitch={canJoinNative}
              disabled={busy === "actor-add"}
              onChange={(next) => {
                if (next === createCategory) return;
                if (createCategory !== "native") {
                  createDrafts.current[createCategory] = {
                    runtime,
                    command,
                    useDefaultCommand,
                    runtimeMode,
                    useProfile,
                    profileId,
                  };
                }
                setCreateCategory(next);
                if (next === "native") return;
                const draft = createDrafts.current[next];
                const nextRuntime: SupportedRuntime =
                  draft?.runtime || (next === "web" ? "web_model" : "codex");
                onChangeUseProfile(draft?.useProfile || false);
                onChangeProfileId(draft?.profileId || "");
                onChangeRuntime(nextRuntime);
                onChangeCommand(
                  draft?.command ??
                    runtimes.find((item) => item.name === nextRuntime)?.recommended_command ??
                    "",
                );
                onChangeUseDefaultCommand(draft?.useDefaultCommand ?? true);
                setRuntimeMode(draft?.runtimeMode || "default");
              }}
            />
          </Surface>
          {createCategory === "native" ? (
            <Surface className={sectionCardClass}>
              <p className="text-sm text-[var(--color-text-secondary)]">
                {t("runtimeSwitch.joinNativeHint")}
              </p>
              <Button
                type="button"
                className="mt-4"
                disabled={!canJoinNative || !groupId}
                onClick={() => setLocalDialogOpen(true)}
              >
                {t("runtimeSwitch.joinNative")}
              </Button>
            </Surface>
          ) : (
            <>
              <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.08fr)_minmax(22rem,0.92fr)] xl:items-start">
                <Surface className={sectionCardClass}>
                  <div className={sectionTitleClass}>{t("sectionBasics")}</div>

                  <div className="mt-4 space-y-4">
                    <div className="grid grid-cols-1 gap-4 sm:grid-cols-[88px_minmax(0,1fr)] sm:items-start">
                      <ActorAvatarField
                        label={null}
                        avatarUrl={undefined}
                        previewUrl={avatarPreviewUrl}
                        runtime={previewRuntime}
                        title={previewTitle}
                        isDark={isDark}
                        sizeClassName="h-16 w-16 sm:h-[4.5rem] sm:w-[4.5rem]"
                        disabled={busy === "actor-add"}
                        resetDisabled={!avatarFile}
                        onSelectFile={setAvatarFile}
                        onReset={() => setAvatarFile(null)}
                      />

                      <div className="min-w-0 space-y-4">
                        <div>
                          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                            {t("agentId", { defaultValue: "Agent ID" })}
                          </label>
                          <Input
                            value={actorId}
                            onChange={(e) => onChangeActorId(e.target.value)}
                            placeholder={suggestedActorId}
                            disabled={busy === "actor-add"}
                          />
                        </div>

                        <div>
                          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                            {t("agentRole", { defaultValue: "Agent role" })}
                          </label>
                          <div className="grid grid-cols-2 gap-2">
                            <Button
                              type="button"
                              variant="outline"
                              className={modeButtonClass(role === "peer")}
                              onClick={() => onChangeRole("peer")}
                              disabled={busy === "actor-add"}
                            >
                              {t("peerRole")}
                            </Button>
                            <Button
                              type="button"
                              variant="outline"
                              className={modeButtonClass(role === "foreman")}
                              onClick={() => onChangeRole("foreman")}
                              disabled={busy === "actor-add" || hasForeman}
                              title={
                                hasForeman
                                  ? t("foremanAlreadyConfigured", {
                                      defaultValue: "Foreman already configured",
                                    })
                                  : undefined
                              }
                            >
                              {t("foremanRole")}
                            </Button>
                          </div>
                          {!hasForeman ? (
                            <div className="text-xs mt-1.5 text-[var(--color-text-muted)]">
                              {t("firstAgentForeman")}
                            </div>
                          ) : null}
                        </div>
                      </div>
                    </div>

                    {attachedScopes.length > 0 || defaultScopeKey || groupId ? (
                      <ActorWorkingDirectory
                        key={`${groupId}/${isOpen}`}
                        groupId={groupId}
                        scopes={attachedScopes}
                        activeScopeKey={activeScopeKey}
                        value={defaultScopeKey}
                        onChange={setDefaultScopeKey}
                        onScopeAttached={onScopeAttached}
                        onBusyChange={setDirectoryBusy}
                        disabled={busy === "actor-add"}
                        hint={defaultScopeKey ? t("workingDirectoryHint") : undefined}
                      />
                    ) : null}

                    <div>
                      <RolePresetPicker
                        showDescription={false}
                        draftValue={actorNotes}
                        onChangeDraft={onChangeActorNotes}
                        disabled={busy === "actor-add"}
                      />
                      <label className="block text-xs font-medium mt-3 mb-2 text-[var(--color-text-muted)]">
                        {t("actorNotes")}
                      </label>
                      <Textarea
                        className="min-h-[144px]"
                        value={actorNotes}
                        onChange={(e) => onChangeActorNotes(e.target.value)}
                        placeholder={t("actorNotesPlaceholder")}
                        spellCheck={false}
                        disabled={busy === "actor-add"}
                      />
                    </div>
                  </div>
                </Surface>

                <Surface className={sectionCardClass}>
                  <div className={sectionTitleClass}>
                    {t("sectionRuntime", "Runtime & Profile")}
                  </div>

                  <div className="mt-4">
                    <RuntimeConfigurationModePicker
                      value={useProfile ? "profile" : "custom"}
                      disabled={busy === "actor-add"}
                      onChange={(mode) => {
                        const nextUsesProfile = mode === "profile";
                        onChangeUseProfile(nextUsesProfile);
                        if (!nextUsesProfile) return;
                        if (!actorProfilesBusy && selectableActorProfiles.length <= 0)
                          void onRequestActorProfiles?.();
                        if (selectableActorProfiles.length > 0 && !profileId)
                          onChangeProfileId(actorProfileIdentityKey(selectableActorProfiles[0]));
                      }}
                    />
                  </div>

                  <div className="mt-4 space-y-4">
                    {useProfile ? (
                      <RuntimeProfilePicker
                        value={profileId}
                        profiles={selectableActorProfiles}
                        busy={actorProfilesBusy}
                        disabled={busy === "actor-add"}
                        onChange={onChangeProfileId}
                      />
                    ) : (
                      <div className="space-y-4">
                        <div>
                          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                            {t("runtime")}
                          </label>
                          <RuntimeSelector
                            showHint={false}
                            className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                            value={runtime}
                            runtimeMode={runtimeMode}
                            disabled={busy === "actor-add"}
                            allowedRuntimes={SUPPORTED_RUNTIMES.filter(
                              (item) => isWebModelRuntime(item) === (createCategory === "web"),
                            )}
                            onChange={(value) => {
                              const next = value as SupportedRuntime;
                              onChangeRuntime(next);
                              if (next === "custom") onChangeUseDefaultCommand(false);
                              else onChangeUseDefaultCommand(true);
                              const nextInfo = runtimes.find((r) => r.name === next);
                              onChangeCommand(String(nextInfo?.recommended_command || "").trim());
                            }}
                            ariaLabel={t("runtime")}
                          />
                          <OpenCodeManagedModelHint runtime={runtime} />
                          {supportsAcpMode(runtime) && (
                            <AcpRuntimeMode
                              runtime={runtime}
                              value={runtimeMode}
                              onChange={setRuntimeMode}
                              disabled={busy === "actor-add"}
                            />
                          )}
                        </div>

                        {runtime === "deepseek" ? (
                          <DeepSeekConnectionFields
                            changes={deepSeekChanges}
                            initialValues={initialSecrets}
                            onChange={setDeepSeekChanges}
                            disabled={busy === "actor-add"}
                          />
                        ) : null}
                        <DeepSeekCommandDetails runtime={runtime}>
                          <RuntimeCommandControl
                            runtime={runtime}
                            command={command}
                            defaultCommand={defaultCommand}
                            useDefaultCommand={useDefaultCommand}
                            disabled={busy === "actor-add"}
                            onCommandChange={onChangeCommand}
                            onUseDefaultCommandChange={onChangeUseDefaultCommand}
                          />
                        </DeepSeekCommandDetails>

                        {webModelSetupIsActorBound ? (
                          <div
                            className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/20 bg-sky-500/5 text-sky-700 dark:text-sky-300"
                            data-status-tone="info"
                          >
                            <div className="font-medium">
                              {runtime === "grok_web_model"
                                ? "Grok Bot Web Model"
                                : runtime === "chatgpt_dots"
                                  ? "ChatGPT Dots"
                                  : t("webModelActorBoundConnectorTitle", {
                                      defaultValue: "ChatGPT Web Model",
                                    })}
                            </div>
                            <div className="mt-1">
                              {runtime === "grok_web_model"
                                ? t("settings:grokActor.hint")
                                : runtime === "chatgpt_dots"
                                  ? t("settings:dotsActor.createHint")
                                  : t("webModelActorBoundConnectorHint", {
                                      defaultValue:
                                        "Use shared login and one connector in global Web Model settings. After saving this Actor, pair its own conversation below.",
                                    })}
                            </div>
                          </div>
                        ) : null}
                      </div>
                    )}
                  </div>
                </Surface>
              </div>

              <Surface className={sectionCardClass}>
                <div className={sectionTitleClass}>{t("sectionAdvanced")}</div>
                <div className="mt-4">
                  <ActorConfigTabs
                    ariaLabel={t("sectionAdvanced")}
                    activeId={activeAdvancedTab || ""}
                    onChange={selectAdvancedTab}
                    tabs={[
                      ...(showRuntimeSetup
                        ? [
                            {
                              id: "connection",
                              label: t("runtimeSetupSection"),
                              panel: (
                                <div
                                  className="rounded-xl border px-3 py-2 text-[11px] border-amber-500/20 bg-amber-500/5 text-amber-700 dark:text-amber-400"
                                  data-status-tone="warning"
                                >
                                  <div className="font-medium">{t("manualMcpRequired")}</div>
                                  <div className="mt-1">
                                    {t("configureMcpStdio")}{" "}
                                    <code className="px-1 rounded bg-amber-500/15">cccc</code>{" "}
                                    {t("thatRuns")}{" "}
                                    <code className="px-1 rounded bg-amber-500/15">cccc mcp</code>.
                                  </div>
                                  <pre className="mt-1.5 p-2 rounded overflow-x-auto whitespace-pre bg-amber-500/10 text-amber-800 dark:text-amber-100">
                                    <code>{BASIC_MCP_CONFIG_SNIPPET}</code>
                                  </pre>
                                  <div className="mt-1 text-[10px] text-amber-700/80 dark:text-amber-400/80">
                                    {t("restartAfterConfig")}
                                  </div>
                                </div>
                              ),
                            },
                          ]
                        : []),
                      ...(!useProfile && !isWebModelRuntime(runtime)
                        ? [
                            {
                              id: "environment",
                              label: t("secretsSection"),
                              panel: (
                                <div>
                                  <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                                    {t("secretsWriteOnly")}
                                  </label>
                                  <Textarea
                                    className="min-h-[96px] font-mono"
                                    value={secretsSetText}
                                    onChange={(e) => onChangeSecretsSetText(e.target.value)}
                                    placeholder={secretsPlaceholder}
                                    disabled={busy === "actor-add"}
                                  />
                                  <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                                    {t("secretsStoredLocally").replace(/<1>|<\/1>/g, "")}
                                  </div>
                                  <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">
                                    {t("secretsFormat").replace(/<1>|<\/1>|<2>|<\/2>/g, "")}
                                  </div>
                                </div>
                              ),
                            },
                          ]
                        : []),
                      ...(previewRuntime
                        ? [
                            {
                              id: "capabilities",
                              label: t("capabilitiesSection"),
                              panel: (
                                <CapabilityPicker
                                  isDark={isDark}
                                  value={parseCapabilityIdInput(capabilityAutoloadText)}
                                  onChange={(next) =>
                                    onChangeCapabilityAutoloadText(formatCapabilityIdInput(next))
                                  }
                                  disabled={busy === "actor-add"}
                                  active={
                                    capabilitiesPrimed || activeAdvancedTab === "capabilities"
                                  }
                                  label={t("autoloadCapabilities")}
                                  hint={t("autoloadCapabilitiesHint")}
                                />
                              ),
                            },
                          ]
                        : []),
                      ...(!useProfile
                        ? [
                            {
                              id: "profile",
                              label: t("profileToolsSection"),
                              panel: (
                                <div className="flex flex-wrap gap-3">
                                  {isWebModelRuntime(runtime) && (
                                    <p className="w-full text-sm text-[var(--color-text-secondary)]">
                                      {t("webModelProfileHint")}
                                    </p>
                                  )}
                                  <Button
                                    type="button"
                                    variant="secondary"
                                    onClick={() =>
                                      void onSaveAsProfile(
                                        runtime === "deepseek" ? deepSeekSaveChanges : undefined,
                                        supportsAcpMode(runtime) ? runtimeMode : "default",
                                      )
                                    }
                                    disabled={
                                      busy === "actor-profile-save" ||
                                      busy === "actor-add" ||
                                      Boolean(deepSeekError)
                                    }
                                  >
                                    {busy === "actor-profile-save"
                                      ? t("savingProfile")
                                      : t("addToActorProfiles")}
                                  </Button>
                                </div>
                              ),
                            },
                          ]
                        : []),
                    ]}
                  />
                </div>
              </Surface>
            </>
          )}
        </div>
      </div>
      {localDialogOpen
        ? createPortal(
            <LocalSessionsDialog
              isDark={isDark}
              initialTargetGroupId={groupId}
              onClose={() => setLocalDialogOpen(false)}
              onSelectGroup={(id) => {
                if (onSelectGroup) onSelectGroup(id);
                else useGroupStore.getState().setSelectedGroupId(id);
                handleCancel();
              }}
            />,
            document.body,
          )
        : null}
    </ModalFrame>
  );
}

export function ActorConfigModal(props: ActorConfigModalProps) {
  if (props.mode === "create") {
    return <CreateActorConfigModal {...props} />;
  }
  return <EditActorConfigModal {...props} />;
}

function EditActorConfigModal({
  isOpen,
  initialSection,
  isDark,
  busy,
  mode: _mode,
  groupId,
  actorId,
  groupRole,
  avatarUrl,
  hasCustomAvatar = false,
  scopes = [],
  activeScopeKey,
  onScopeAttached,
  isRunning,
  savedRuntime,
  savedActor,
  savedActorNotes,
  runtimes,
  runtime,
  onChangeRuntime,
  command,
  onChangeCommand,
  title,
  onChangeTitle,
  actorNotes,
  onChangeActorNotes,
  actorNotesBusy = false,
  actorNotesError = false,
  onRetryActorNotes,
  capabilityAutoloadText,
  onChangeCapabilityAutoloadText,
  onSave,
  onSaveAndRestart,
  onSaveBasics,
  linkedProfileId,
  linkedProfileScope,
  linkedProfileOwner,
  actorProfiles,
  actorProfilesBusy,
  onRequestActorProfiles,
  onSaveAsProfile,
  onAvatarChanged,
  inlineNotice,
  onCancel,
}: EditActorConfigProps) {
  const { t } = useTranslation("actors");
  const conversationRef = useRef<HTMLDivElement>(null);
  const sharedSettingsTriggerRef = useRef<HTMLElement | null>(null);
  const settingsOpen = useModalStore((state) => state.modals.settings);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const canAccessGlobalSettings = useUIStore((state) => state.canAccessGlobalSettings !== false);
  const [sharedSettingsRequested, setSharedSettingsRequested] = useState(false);
  const currentCategory = actorRuntimeCategory(
    savedActor || { id: actorId, runtime: savedRuntime },
  );
  const isNativeActor = currentCategory === "native";
  const [category, setCategory] = useState<RuntimeCategory>(currentCategory);
  const [nativeSwitchVisited, setNativeSwitchVisited] = useState(isNativeActor);
  const [managedSwitchVisited, setManagedSwitchVisited] = useState(false);
  const [webSwitchVisited, setWebSwitchVisited] = useState(false);
  const [sourceDirectory, setSourceDirectory] = useState("");
  const canSwitch = useUIStore((state) => state.isWebAdmin === true && !state.webReadOnly);
  const switchPending = useGroupStore((state) =>
    savedActor ? hasPendingActorSwitch(state.groupDoc, savedActor, groupId) : false,
  );
  const suspended = sharedSettingsRequested && settingsOpen;
  const focusConversation = initialSection === "chatgpt" && isWebModelRuntime(savedRuntime);
  const { modalRef } = useModalA11y(isOpen && !suspended, closeEdit, {
    initialFocusRef: focusConversation ? conversationRef : undefined,
  });
  useChatObscuringView(isOpen);
  useEffect(() => {
    setSharedSettingsRequested(false);
  }, [groupId, actorId, isOpen]);
  useEffect(() => {
    if (!isOpen || suspended || !sharedSettingsRequested) return;
    const modal = modalRef.current;
    const overlay = modal?.parentElement;
    let restored = false;
    let focusFrame = 0;
    const restoreFocus = () => {
      const trigger = sharedSettingsTriggerRef.current;
      if (restored || !modal || !trigger || !modal.contains(trigger)) return;
      // The overlay can remain hidden throughout its visibility transition.
      if (getComputedStyle(modal).visibility !== "visible") return;
      restored = true;
      overlay?.removeEventListener("transitionend", onTransitionEnd);
      trigger.focus();
    };
    const onTransitionEnd = (event: Event) => {
      if (event.target !== overlay) return;
      cancelAnimationFrame(focusFrame);
      focusFrame = requestAnimationFrame(restoreFocus);
    };
    overlay?.addEventListener("transitionend", onTransitionEnd);
    focusFrame = requestAnimationFrame(restoreFocus);
    return () => {
      cancelAnimationFrame(focusFrame);
      overlay?.removeEventListener("transitionend", onTransitionEnd);
    };
  }, [actorId, groupId, isOpen, modalRef, sharedSettingsRequested, suspended]);
  const openSharedSettings = () => {
    const trigger = document.activeElement;
    sharedSettingsTriggerRef.current =
      trigger instanceof HTMLElement && modalRef.current?.contains(trigger) ? trigger : null;
    setSharedSettingsRequested(true);
    openSettingsTarget({
      scope: "global",
      tab: "webModels",
      webModelProvider: conversationRuntime === "grok_web_model" ? "grok_web" : "chatgpt_web",
      webModelSection: conversationRuntime === "web_model" ? "login" : "connector",
    });
  };
  const [secretKeys, setSecretKeys] = useState<string[]>([]);
  const [secretMasks, setSecretMasks] = useState<Record<string, string>>({});
  const [secretChanges, setSecretChanges] = useState<ActorSecretChanges>(emptyActorSecretChanges);
  const [secretsError, setSecretsError] = useState("");
  const [secretsBusy, setSecretsBusy] = useState(false);
  const [secretsRefreshing, setSecretsRefreshing] = useState(false);
  const [secretEditorOpen, setSecretEditorOpen] = useState(false);
  const [secretKeysLoadFailed, setSecretKeysLoadFailed] = useState(false);
  const [attachProfileId, setAttachProfileId] = useState("");
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode>("default");
  const [managedSwitchPayload, setManagedSwitchPayload] = useState<EditActorSavePayload | null>(
    null,
  );
  const [managedSwitchNoStart, setManagedSwitchNoStart] = useState(false);
  const [managedProfileBusy, setManagedProfileBusy] = useState(false);
  const [managedProfileSaveFailed, setManagedProfileSaveFailed] = useState(false);
  const [managedProfileMessage, setManagedProfileMessage] = useState("");
  const [managedSwitchBlocked, setManagedSwitchBlocked] = useState(false);
  const [managedSwitchDone, setManagedSwitchDone] = useState(false);
  const [webSetupRuntime, setWebSetupRuntime] = useState("");
  const savedDefaultScopeKey = savedActor?.default_scope_key || "";
  const [defaultScopeKey, setDefaultScopeKey] = useState(savedDefaultScopeKey);
  const [directoryBusy, setDirectoryBusy] = useState(false);
  const attachedScopes = scopes.filter((scope) => scope.scope_key && scope.url);
  const scopeUnavailable = Boolean(
    category !== "web" &&
    defaultScopeKey &&
    !attachedScopes.some((scope) => scope.scope_key === defaultScopeKey),
  );
  const [editMode, setEditMode] = useState<ConfigMode>("custom");
  const [pendingConvertToCustom, setPendingConvertToCustom] = useState(false);
  const lastNonNativeCategory = useRef<"managed" | "web">(
    currentCategory === "web" ? "web" : "managed",
  );
  const categoryDrafts = useRef<
    Partial<
      Record<
        "managed" | "web",
        {
          runtime: SupportedRuntime;
          command: string;
          runtimeMode: RuntimeMode;
          editMode: ConfigMode;
          pendingConvertToCustom: boolean;
          attachProfileId: string;
        }
      >
    >
  >({});
  useEffect(() => {
    if (isOpen) {
      const current = actorRuntimeCategory(savedActor || { id: actorId, runtime: savedRuntime });
      setCategory(current);
      setNativeSwitchVisited(current === "native");
      setManagedSwitchVisited(false);
      setWebSwitchVisited(false);
      lastNonNativeCategory.current = current === "web" ? "web" : "managed";
      categoryDrafts.current = {};
      setSourceDirectory("");
    }
    // The category and its drafts belong to one editor visit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupId, actorId, isOpen]);
  const [grokBotUrl, setGrokBotUrl] = useState<string | undefined>();
  const [connectionBusy, setConnectionBusy] = useState(false);
  const [connectionEnabled, setConnectionEnabled] = useState<boolean | undefined>();
  const [connectionUnresolved, setConnectionUnresolved] = useState(false);
  const baseline = useRef({
    identity: "",
    title,
    command,
    actorNotes,
    notesLoaded: false,
    capabilityAutoloadText,
  });
  const identity = isOpen ? `${groupId}/${actorId}` : "";
  const avatarVisitRef = useRef({ identity });
  if (avatarVisitRef.current.identity !== identity) avatarVisitRef.current = { identity };
  if (baseline.current.identity !== identity) {
    baseline.current = {
      identity,
      title,
      command,
      actorNotes,
      notesLoaded: !actorNotesBusy,
      capabilityAutoloadText,
    };
  }
  if (actorNotesBusy) baseline.current.notesLoaded = false;
  if (!baseline.current.notesLoaded && !actorNotesBusy) {
    baseline.current.actorNotes = actorNotes;
    baseline.current.notesLoaded = true;
  }
  // Successful steps may persist before a later binding/restart step fails.
  // Refresh the comparison baseline without replacing the user's form draft.
  if (savedActor) {
    baseline.current.title = savedActor.title || "";
    baseline.current.command = formatRuntimeCommand(savedActor.command);
    baseline.current.capabilityAutoloadText = formatCapabilityIdInput(
      savedActor.capability_autoload,
    );
  }
  if (savedActorNotes !== undefined && !actorNotesBusy) {
    baseline.current.actorNotes = savedActorNotes;
  }
  useEffect(() => {
    setGrokBotUrl(undefined);
    setConnectionBusy(false);
    setConnectionEnabled(undefined);
    setConnectionUnresolved(false);
  }, [groupId, actorId, isOpen]);
  const [localNotice, setLocalNotice] = useState("");
  const [avatarBusy, setAvatarBusy] = useState<"" | "upload" | "clear">("");
  const [capabilitiesPrimed, setCapabilitiesPrimed] = useState(false);
  const [advancedTab, setAdvancedTab] = useState<AdvancedTabId>("environment");
  const secretFetchSeqRef = useRef(0);
  const initializedDraftIdentityRef = useRef("");
  const modalStateRef = useRef<{
    groupId: string;
    actorId: string;
    linked: boolean;
    editMode: ConfigMode;
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
  const selectableActorProfiles = useMemo(
    () =>
      category === "native"
        ? actorProfiles
        : actorProfiles.filter(
            (profile) => isWebModelRuntime(profile.runtime) === (category === "web"),
          ),
    [actorProfiles, category],
  );
  const selectedProfile = useMemo(
    () =>
      selectableActorProfiles.find(
        (profile) => actorProfileIdentityKey(profile) === String(attachProfileId || "").trim(),
      ),
    [selectableActorProfiles, attachProfileId],
  );
  const conversationRuntime =
    editMode === "profile" ? selectedProfile?.runtime || runtime : runtime;
  const stagingGrok =
    conversationRuntime === "grok_web_model" && conversationRuntime !== savedRuntime;
  const grokTargetChanged =
    conversationRuntime === "grok_web_model" && (stagingGrok || grokBotUrl !== undefined);
  const applyRunningTarget = grokTargetChanged && (connectionEnabled ?? isRunning);
  const launchChanges = buildActorSecretSaveChanges(secretChanges);
  const hasSecretChanges =
    launchChanges.clear ||
    launchChanges.unsetKeys.length > 0 ||
    Object.keys(launchChanges.setVars).length > 0;
  const basicsChanged =
    title.trim() !== baseline.current.title.trim() ||
    (baseline.current.notesLoaded && actorNotes.trim() !== baseline.current.actorNotes.trim());
  const otherChanges =
    scopeUnavailable ||
    hasSecretChanges ||
    defaultScopeKey !== savedDefaultScopeKey ||
    JSON.stringify(parseCapabilityIdInput(capabilityAutoloadText)) !==
      JSON.stringify(parseCapabilityIdInput(baseline.current.capabilityAutoloadText)) ||
    grokTargetChanged ||
    pendingConvertToCustom ||
    (editMode === "custom" &&
      (linked ||
        runtime !== savedRuntime ||
        (supportsAcpMode(runtime) ? runtimeMode : "default") !==
          (savedActor?.runtime_mode || "default") ||
        (!isWebModelRuntime(runtime) && command.trim() !== baseline.current.command.trim()))) ||
    (editMode === "profile" &&
      (!linked ||
        attachProfileId !==
          actorProfileIdentityKey({
            id: linkedProfileId || "",
            scope: linkedProfileScope || "global",
            owner_id: linkedProfileOwner || "",
          })));
  const hasChanges = basicsChanged || otherChanges || secretEditorOpen;
  const showRuntimeSetup = !effectiveLinked && editMode === "custom" && runtime === "custom";
  const editAdvancedTabIds: AdvancedTabId[] = [
    ...(showRuntimeSetup ? ["connection" as const] : []),
    ...(editMode === "custom" && !isWebModelRuntime(runtime) ? ["environment" as const] : []),
    "capabilities",
    ...(editMode === "custom" ? ["profile" as const] : []),
  ];
  const activeAdvancedTab = editAdvancedTabIds.includes(advancedTab)
    ? advancedTab
    : editAdvancedTabIds[0];
  const selectedProfileName = String(selectedProfile?.name || "").trim();
  const changeCategory = (next: RuntimeCategory) => {
    if (next !== category && secretEditorOpen) {
      setSecretsError(t("secretEditorIncomplete"));
      return;
    }
    if (next === "native") setNativeSwitchVisited(true);
    if (isNativeActor && next === "managed") setManagedSwitchVisited(true);
    if (isNativeActor && next === "web") setWebSwitchVisited(true);
    // A native member's managed target is a separate switch draft. Do not
    // put it in the ordinary saved runtime fields.
    if (isNativeActor) {
      setCategory(next);
      return;
    }
    if (next === "native") {
      if (category !== "native") lastNonNativeCategory.current = category;
      setCategory(next);
      return;
    }
    const previous = category === "native" ? lastNonNativeCategory.current : category;
    if (previous !== next) {
      categoryDrafts.current[previous] = {
        runtime,
        command,
        runtimeMode,
        editMode,
        pendingConvertToCustom,
        attachProfileId,
      };
      const draft = categoryDrafts.current[next];
      if (draft) {
        onChangeRuntime(draft.runtime);
        onChangeCommand(draft.command);
        setRuntimeMode(draft.runtimeMode);
        setEditMode(draft.editMode);
        setPendingConvertToCustom(draft.pendingConvertToCustom);
        setAttachProfileId(draft.attachProfileId);
      } else {
        const candidate = next === "web" ? "web_model" : "codex";
        if (linked) setPendingConvertToCustom(true);
        setEditMode("custom");
        onChangeRuntime(candidate);
        if (next === "managed") {
          onChangeCommand(
            runtimes.find((item) => item.name === candidate)?.recommended_command || "",
          );
        }
      }
    }
    lastNonNativeCategory.current = next;
    setCategory(next);
  };
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
    if (editMode !== "custom" || isWebModelRuntime(runtime)) {
      setSecretsRefreshing(false);
      setSecretKeysLoadFailed(false);
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }

    if (linked && pendingConvertToCustom) {
      const profileId = String(linkedProfileId || "").trim();
      if (!profileId) {
        setSecretsRefreshing(false);
        setSecretKeysLoadFailed(false);
        setSecretKeys([]);
        setSecretMasks({});
        return;
      }
      const requestSeq = ++secretFetchSeqRef.current;
      setSecretsRefreshing(true);
      setSecretKeysLoadFailed(false);
      const resp = await (async () => {
        try {
          return await api.fetchActorProfilePrivateEnvKeys(profileId, {
            scope: linkedProfileScope,
            ownerId: linkedProfileOwner,
          });
        } finally {
          if (requestSeq === secretFetchSeqRef.current) setSecretsRefreshing(false);
        }
      })();
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
        setSecretKeysLoadFailed(true);
        setSecretKeys([]);
        setSecretMasks({});
        return;
      }

      const loaded = normalizeLoadedActorSecretKeys(resp.result);
      setSecretsError(loaded.error);
      setSecretKeysLoadFailed(loaded.loadFailed);
      setSecretKeys(loaded.keys);
      setSecretMasks(loaded.masks);
      return;
    }

    if (effectiveLinked) {
      setSecretsRefreshing(false);
      setSecretKeysLoadFailed(false);
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }

    if (!groupId || !actorId) {
      setSecretsRefreshing(false);
      setSecretKeysLoadFailed(false);
      return;
    }
    const requestForGroupId = groupId;
    const requestForActorId = actorId;
    const requestSeq = ++secretFetchSeqRef.current;
    setSecretsRefreshing(true);
    setSecretKeysLoadFailed(false);
    const resp = await (async () => {
      try {
        return await api.fetchActorPrivateEnvKeys(requestForGroupId, requestForActorId);
      } finally {
        if (requestSeq === secretFetchSeqRef.current) setSecretsRefreshing(false);
      }
    })();
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
        setSecretKeysLoadFailed(false);
        setSecretKeys([]);
        setSecretMasks({});
        return;
      }
      setSecretsError(resp.error?.message || t("failedToLoadSecrets"));
      setSecretKeysLoadFailed(true);
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }
    const loaded = normalizeLoadedActorSecretKeys(resp.result);
    setSecretsError(loaded.error);
    setSecretKeysLoadFailed(loaded.loadFailed);
    setSecretKeys(loaded.keys);
    setSecretMasks(loaded.masks);
  };

  useEffect(() => {
    if (!isOpen) {
      initializedDraftIdentityRef.current = "";
      return;
    }
    const hasLinked = Boolean(String(linkedProfileId || "").trim());
    if (initializedDraftIdentityRef.current === identity) {
      // A partial save updates the persisted Profile link, not the edit session.
      if (!hasLinked) setPendingConvertToCustom(false);
      return;
    }
    initializedDraftIdentityRef.current = identity;
    setRuntimeMode(savedActor?.runtime_mode || "default");
    setManagedSwitchPayload(null);
    setManagedSwitchBlocked(false);
    setManagedSwitchDone(false);
    setWebSetupRuntime("");
    setDefaultScopeKey(savedDefaultScopeKey);
    secretFetchSeqRef.current += 1;
    setEditMode(hasLinked ? "profile" : "custom");
    setPendingConvertToCustom(false);
    setAttachProfileId(
      hasLinked
        ? actorProfileIdentityKey({
            id: String(linkedProfileId || "").trim(),
            scope: linkedProfileScope || "global",
            owner_id: linkedProfileOwner || "",
          })
        : "",
    );
    setLocalNotice("");
    setAvatarBusy("");
    setSecretsError("");
    setSecretsRefreshing(false);
    setSecretKeysLoadFailed(false);
    setSecretMasks({});
    setSecretChanges(emptyActorSecretChanges());
    setSecretKeys([]);
  }, [
    identity,
    isOpen,
    linkedProfileId,
    linkedProfileOwner,
    linkedProfileScope,
    savedActor?.runtime_mode,
    savedDefaultScopeKey,
  ]);

  useEffect(() => {
    if (!isOpen) return;
    if (isNativeActor || editMode === "profile" || isWebModelRuntime(runtime)) {
      secretFetchSeqRef.current += 1;
      setSecretsRefreshing(false);
      setSecretKeysLoadFailed(false);
      setSecretKeys([]);
      setSecretMasks({});
      return;
    }
    if (!effectiveLinked) void refreshSecretKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen, groupId, actorId, editMode, effectiveLinked, runtime, isNativeActor]);

  useEffect(() => {
    if (!isOpen) return;
    setCapabilitiesPrimed(false);
  }, [isOpen, groupId, actorId]);

  if (!isOpen) return null;

  const rtInfo = runtimes.find((r) => r.name === runtime);
  const available = rtInfo?.available ?? false;
  const defaultCommand = rtInfo?.recommended_command || "";
  const deepSeekError =
    runtime === "deepseek" && editMode === "custom"
      ? deepSeekSettingsError(secretChanges.setVars)
      : "";
  const requireCommand =
    !effectiveLinked &&
    editMode === "custom" &&
    !isWebModelRuntime(runtime) &&
    (runtime === "custom" || (!available && !(runtime === "antigravity" && runtimeMode === "acp")));
  const selectAdvancedTab = (id: string) => {
    const next = id as AdvancedTabId;
    setAdvancedTab(next);
    if (next === "capabilities") setCapabilitiesPrimed(true);
  };

  const convertToCustomDraft = () => {
    if (!linked || busy === "actor-update") return;
    setSecretsError("");
    setLocalNotice("");
    setPendingConvertToCustom(true);
    setEditMode("custom");
    setRuntimeMode(savedActor?.runtime_mode || "default");
  };

  const saveAsProfile = async () => {
    if (deepSeekError) {
      setSecretsError(t(`deepseek.${deepSeekError}`));
      return;
    }
    if (secretEditorOpen) {
      setSecretsError(t("secretEditorIncomplete"));
      return;
    }
    if (isWebModelRuntime(runtime) && hasSecretChanges) {
      setSecretsError(t("secretDraftNotApplied"));
      return;
    }
    setSecretsError("");
    setLocalNotice("");
    setSecretsBusy(true);
    try {
      const result = await onSaveAsProfile(
        buildActorSecretSaveChanges(
          isWebModelRuntime(runtime)
            ? emptyActorSecretChanges()
            : runtime === "deepseek"
              ? normalizeDeepSeekSettings(secretChanges)
              : secretChanges,
        ),
        supportsAcpMode(runtime) ? runtimeMode : "default",
      );
      const profileId = String(result?.profileId || "").trim();
      if (profileId && result?.useNow) {
        setSecretChanges(emptyActorSecretChanges());
        setPendingConvertToCustom(false);
        setEditMode("profile");
        setAttachProfileId(
          actorProfileIdentityKey({ id: profileId, scope: "global", owner_id: "" }),
        );
        setLocalNotice(
          t("profileSelectedPendingSave", { name: String(result.profileName || profileId) }),
        );
      }
    } catch (e) {
      setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
    } finally {
      setSecretsBusy(false);
    }
  };

  const handleUploadAvatar = async (file: File | null) => {
    if (!file || !groupId || !actorId) return;
    const visit = avatarVisitRef.current;
    setAvatarBusy("upload");
    setSecretsError("");
    setLocalNotice("");
    try {
      const resp = await api.uploadActorAvatar(groupId, actorId, file);
      if (avatarVisitRef.current !== visit) return;
      if (!resp.ok) {
        setSecretsError(resp.error?.message || t("avatarUploadFailed"));
        return;
      }
      const adopted = await onAvatarChanged?.(resp.result.actor, savedActor);
      if (avatarVisitRef.current !== visit) return;
      if (adopted === false) {
        setManagedSwitchBlocked(true);
        setSecretsError(joinRuntimeSentences([t("avatarSaved"), t("runtimeSwitch.conflict")]));
        return;
      }
      setLocalNotice(t("avatarSaved"));
    } catch {
      if (avatarVisitRef.current === visit) setSecretsError(t("avatarUploadFailed"));
    } finally {
      if (avatarVisitRef.current === visit) setAvatarBusy("");
    }
  };

  const handleClearAvatar = async () => {
    if (!groupId || !actorId) return;
    const visit = avatarVisitRef.current;
    setAvatarBusy("clear");
    setSecretsError("");
    setLocalNotice("");
    try {
      const resp = await api.clearActorAvatar(groupId, actorId);
      if (avatarVisitRef.current !== visit) return;
      if (!resp.ok) {
        setSecretsError(resp.error?.message || t("saveFailed"));
        return;
      }
      const adopted = await onAvatarChanged?.(resp.result.actor, savedActor);
      if (avatarVisitRef.current !== visit) return;
      if (adopted === false) {
        setManagedSwitchBlocked(true);
        setSecretsError(joinRuntimeSentences([t("avatarReset"), t("runtimeSwitch.conflict")]));
        return;
      }
      setLocalNotice(t("avatarReset"));
    } catch {
      if (avatarVisitRef.current === visit) setSecretsError(t("saveFailed"));
    } finally {
      if (avatarVisitRef.current === visit) setAvatarBusy("");
    }
  };

  const submit = async (restart: boolean) => {
    if (!groupId || !actorId) return;
    if (busy === "actor-update" || scopeUnavailable || directoryBusy) return;
    if (deepSeekError) {
      setSecretsError(t(`deepseek.${deepSeekError}`));
      return;
    }
    if (secretEditorOpen) {
      setSecretsError(t("secretEditorIncomplete"));
      return;
    }
    const callback = restart ? onSaveAndRestart : onSave;
    const nextRuntime = editMode === "profile" ? selectedProfile?.runtime : runtime;
    if (hasSecretChanges && (editMode === "profile" || isWebModelRuntime(nextRuntime))) {
      setSecretsError(t("secretDraftNotApplied"));
      return;
    }
    const profileChanged =
      editMode === "profile" &&
      (!linked ||
        attachProfileId !==
          actorProfileIdentityKey({
            id: linkedProfileId || "",
            scope: linkedProfileScope || "global",
            owner_id: linkedProfileOwner || "",
          }));
    const profileRevisionChanged =
      linked &&
      editMode === "profile" &&
      (selectedProfile?.revision === undefined ||
        selectedProfile.revision !== savedActor?.profile_revision_applied);
    const needsSwitch =
      restart &&
      currentCategory !== "native" &&
      nextRuntime &&
      ((linked && editMode === "custom") ||
        profileChanged ||
        profileRevisionChanged ||
        nextRuntime !== savedRuntime ||
        (supportsAcpMode(nextRuntime) ? runtimeMode : "default") !==
          (savedActor?.runtime_mode || "default"));
    const invoke = async (payload: EditActorSavePayload) => {
      if (needsSwitch) {
        if (!canSwitch) {
          setSecretsError(t("runtimeSwitch.notAdmin"));
          return;
        }
        if (managedSwitchBlocked) {
          setSecretsError(t("runtimeSwitch.unknown"));
          return;
        }
        const capabilityChanged =
          JSON.stringify(parseCapabilityIdInput(capabilityAutoloadText)) !==
          JSON.stringify(parseCapabilityIdInput(baseline.current.capabilityAutoloadText));
        if (
          basicsChanged ||
          actorNotesBusy ||
          capabilityChanged ||
          launchChanges.clear ||
          launchChanges.unsetKeys.length ||
          Object.keys(launchChanges.setVars).length
        ) {
          setSecretsError(t("runtimeSwitch.dirty"));
          return;
        }
        if (
          !linked &&
          editMode === "custom" &&
          !isWebModelRuntime(nextRuntime) &&
          (secretsRefreshing || secretKeysLoadFailed)
        ) {
          setSecretsError(t("failedToLoadSecrets"));
          return;
        }
        setManagedSwitchNoStart(linked && editMode === "custom" && !isWebModelRuntime(nextRuntime));
        setManagedProfileSaveFailed(false);
        setManagedProfileMessage("");
        setManagedSwitchPayload(payload);
        return;
      }
      if (
        !restart &&
        currentCategory === "managed" &&
        isWebModelRuntime(nextRuntime) &&
        !linked &&
        baseline.current.command.trim() &&
        !window.confirm(t("runtimeSwitch.managedToWebSaveConfirm"))
      )
        return;
      if (
        !restart &&
        currentCategory === "web" &&
        nextRuntime !== savedRuntime &&
        !window.confirm(t("runtimeSwitch.oldWebSaveConfirm"))
      )
        return;
      await callback(payload);
    };

    if (grokTargetChanged && !isGrokBotUrl(grokBotUrl || "")) {
      setSecretsError(t("settings:grokActor.invalidUrl"));
      return;
    }
    if (
      applyRunningTarget &&
      !stagingGrok &&
      !restart &&
      !window.confirm(t("applyWebModelTargetConfirm", { label: actorId }))
    )
      return;

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
        await invoke({
          mode: "profile",
          setVars: {},
          unsetKeys: [],
          clear: false,
          capabilityAutoload: parseCapabilityIdInput(capabilityAutoloadText),
          defaultScopeKey,
          profileId,
          ...(grokTargetChanged ? { grokBotUrl: grokBotUrl!.trim() } : {}),
        });
      } catch (e) {
        setLocalNotice("");
        setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
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
    const secretSaveChanges = buildActorSecretSaveChanges(
      isWebModelRuntime(runtime)
        ? emptyActorSecretChanges()
        : runtime === "deepseek"
          ? normalizeDeepSeekSettings(secretChanges)
          : secretChanges,
    );

    setSecretsBusy(true);
    try {
      await invoke({
        mode: "custom",
        runtimeMode: supportsAcpMode(runtime) ? runtimeMode : "default",
        setVars: secretSaveChanges.setVars,
        unsetKeys: secretSaveChanges.unsetKeys,
        clear: secretSaveChanges.clear,
        capabilityAutoload: parseCapabilityIdInput(capabilityAutoloadText),
        defaultScopeKey,
        convertToCustom: linked && pendingConvertToCustom,
        onSecretsSaved: (keys) => {
          if (initializedDraftIdentityRef.current !== identity) return;
          secretFetchSeqRef.current += 1;
          setSecretChanges(emptyActorSecretChanges());
          setSecretKeys(keys);
          setSecretMasks({});
          setSecretKeysLoadFailed(false);
          setSecretsRefreshing(false);
        },
        ...(grokTargetChanged ? { grokBotUrl: grokBotUrl!.trim() } : {}),
      });
    } catch (e) {
      setLocalNotice("");
      setSecretsError(e instanceof Error ? e.message : t("saveFailed"));
      return;
    } finally {
      setSecretsBusy(false);
    }
  };

  const sectionCardClass = "min-w-0 rounded-2xl p-4 sm:p-5 glass-panel";
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const saveDisabled =
    Boolean(deepSeekError) ||
    connectionBusy ||
    busy === "actor-update" ||
    avatarBusy !== "" ||
    secretsBusy ||
    secretsRefreshing ||
    actorNotesBusy ||
    actorNotesError ||
    scopeUnavailable ||
    directoryBusy ||
    (grokTargetChanged && !grokBotUrl?.trim()) ||
    (grokTargetChanged && connectionUnresolved) ||
    (editMode === "custom" && effectiveLinked) ||
    (editMode === "custom" && requireCommand && !command.trim()) ||
    (editMode === "profile" && !String(attachProfileId || "").trim());
  function closeEdit() {
    if (busy === "actor-update" || secretsBusy || connectionBusy || avatarBusy !== "") return;
    onCancel();
  }
  function openSwitchedWebSetup(runtime: string) {
    if (!savedActor) return;
    const expected = actorGenerationIdentity(savedActor);
    onCancel();
    window.setTimeout(() => {
      const result = openWebSetup(groupId, actorId, runtime, expected);
      if (result !== "opened")
        useUIStore
          .getState()
          .showNotice({
            message: t(
              result === "editor_open"
                ? "runtimeSwitch.closeEditorFirst"
                : "runtimeSwitch.refreshFailed",
            ),
          });
    }, 0);
  }
  const normalizedGroupRole = normalizeGroupRole(groupRole);
  const groupRoleLabel =
    normalizedGroupRole === "foreman"
      ? t("groupRoleForeman", { defaultValue: "Foreman" })
      : t("groupRolePeer", { defaultValue: "Peer" });
  const switchVisit = avatarVisitRef.current;
  const renderSwitchControls = (targetKind: "native" | "managed" | "web") =>
    savedActor && canSwitch ? (
      <RuntimeSwitchControls
        key={`${groupId}/${actorId}/${targetKind}`}
        groupId={groupId}
        actor={savedActor}
        targetKind={targetKind}
        isDark={isDark}
        dirty={
          isNativeActor
            ? basicsChanged || actorNotesBusy || actorNotesError || savedActorNotes === undefined
            : hasChanges || actorNotesBusy || actorNotesError
        }
        disabled={
          busy === "actor-update" || avatarBusy !== "" || switchPending || managedSwitchBlocked
        }
        onOutcomeUnknown={() => setManagedSwitchBlocked(true)}
        onSwitched={() => {
          setManagedSwitchBlocked(true);
          setManagedSwitchDone(true);
        }}
        onWebSetupReady={(runtime) => {
          if (avatarVisitRef.current === switchVisit) setWebSetupRuntime(runtime);
        }}
        runtimes={runtimes}
        profiles={actorProfiles}
        profilesBusy={actorProfilesBusy}
        scopes={scopes}
        activeScopeKey={activeScopeKey}
        onRequestProfiles={onRequestActorProfiles}
        onCurrentSourceDirectory={isNativeActor ? setSourceDirectory : undefined}
        onSaveCurrentProfile={async () =>
          Boolean(
            (
              await onSaveAsProfile(undefined, savedActor.runtime_mode || "default", {
                savedActor: true,
              })
            )?.profileId,
          )
        }
      />
    ) : null;

  return (
    <ModalFrame
      isOpen={isOpen && !suspended}
      isDark={isDark}
      onClose={closeEdit}
      titleId="edit-actor-title"
      title={
        <div>
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("editAgent", { actorId })}
          </div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      closeDisabled={busy === "actor-update" || secretsBusy || connectionBusy || avatarBusy !== ""}
      panelClassName={ACTOR_MODAL_PANEL_CLASS}
      modalRef={modalRef}
      footerActions={
        <>
          {secretsError ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-rose-500/20 bg-rose-500/5 text-rose-600 dark:text-rose-400"
              role="alert"
              data-status-tone="error"
            >
              {secretsError}
            </div>
          ) : null}

          {String(localNotice || inlineNotice || "").trim() ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
              role="status"
            >
              {String(localNotice || inlineNotice || "").trim()}
            </div>
          ) : null}

          {webSetupRuntime ? (
            <div className="mb-3 flex sm:justify-end">
              <Button
                type="button"
                variant="outline"
                className="w-full sm:w-auto"
                disabled={
                  busy === "actor-update" || secretsBusy || connectionBusy || avatarBusy !== ""
                }
                onClick={() => openSwitchedWebSetup(webSetupRuntime)}
              >
                {t("runtimeSwitch.openWebSetup")}
              </Button>
            </div>
          ) : null}

          {isNativeActor || category === "native" ? (
            <div className="flex flex-col-reverse sm:flex-row gap-3 sm:justify-end">
              <Button
                type="button"
                variant="secondary"
                onClick={closeEdit}
                disabled={busy === "actor-update" || secretsBusy || avatarBusy !== ""}
              >
                {t(managedSwitchDone ? "common:close" : "common:cancel")}
              </Button>
              <Button
                type="button"
                disabled={
                  busy === "actor-update" ||
                  actorNotesBusy ||
                  actorNotesError ||
                  (isNativeActor && savedActorNotes === undefined) ||
                  switchPending ||
                  managedSwitchBlocked ||
                  avatarBusy !== "" ||
                  (!isNativeActor && otherChanges) ||
                  (basicsChanged && !onSaveBasics)
                }
                onClick={() => {
                  if (!basicsChanged) {
                    closeEdit();
                    return;
                  }
                  void onSaveBasics?.().catch((error: unknown) =>
                    setSecretsError(error instanceof Error ? error.message : t("saveFailed")),
                  );
                }}
              >
                {t(basicsChanged ? "common:save" : "common:done")}
              </Button>
            </div>
          ) : (
            <div className="flex flex-col-reverse sm:flex-row gap-3 sm:justify-end">
              {hasChanges && (
                <Button
                  type="button"
                  variant="secondary"
                  className="w-full sm:w-auto transition-all ease-spring duration-300"
                  onClick={closeEdit}
                  disabled={busy === "actor-update" || secretsBusy || connectionBusy}
                >
                  {t("common:cancel")}
                </Button>
              )}
              {(hasChanges || linked) && (!applyRunningTarget || stagingGrok) && (
                <Button
                  type="button"
                  variant="outline"
                  className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
                  onClick={() => void submit(true)}
                  disabled={saveDisabled || switchPending || managedSwitchBlocked}
                >
                  {t("saveAndRestart")}
                </Button>
              )}
              <Button
                type="button"
                className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
                onClick={() => (hasChanges ? void submit(false) : closeEdit())}
                disabled={
                  hasChanges
                    ? saveDisabled || switchPending || managedSwitchBlocked
                    : busy === "actor-update" || secretsBusy || connectionBusy
                }
              >
                {t(
                  !hasChanges
                    ? "common:done"
                    : applyRunningTarget && !stagingGrok
                      ? "saveAndApply"
                      : "common:save",
                )}
              </Button>
            </div>
          )}
        </>
      }
    >
      {savedActor ? <RuntimeSwitchPendingNotice actor={savedActor} groupId={groupId} /> : null}
      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-hide bg-[var(--color-bg-primary)] p-4 sm:p-6">
        <fieldset
          disabled={
            secretsBusy ||
            connectionBusy ||
            busy === "actor-update" ||
            avatarBusy !== "" ||
            managedSwitchBlocked
          }
          aria-busy={busy === "actor-update" || actorNotesBusy || avatarBusy !== ""}
          className="mx-auto w-full max-w-6xl min-w-0 space-y-4"
        >
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.08fr)_minmax(22rem,0.92fr)] xl:items-start">
            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionBasics", "Basics")}</div>

              <div className="mt-4 space-y-4">
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-[88px_minmax(0,1fr)] sm:items-start">
                  <div className="justify-self-start">
                    <ActorAvatarField
                      label={null}
                      avatarUrl={avatarUrl}
                      runtime={runtime}
                      title={title || actorId}
                      isDark={isDark}
                      sizeClassName="h-16 w-16 sm:h-[4.5rem] sm:w-[4.5rem]"
                      disabled={busy === "actor-update" || avatarBusy !== "" || switchPending}
                      resetDisabled={!hasCustomAvatar}
                      uploadBusy={avatarBusy === "upload"}
                      resetBusy={avatarBusy === "clear"}
                      onSelectFile={(file) => void handleUploadAvatar(file)}
                      onReset={() => void handleClearAvatar()}
                    />
                  </div>

                  <div className="min-w-0">
                    <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                      {t("displayName")}
                    </label>
                    <Input
                      value={title}
                      onChange={(e) => onChangeTitle(e.target.value)}
                      placeholder={actorId}
                    />
                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      <span className="text-xs font-medium text-[var(--color-text-muted)]">
                        {t("groupRole", { defaultValue: "Group role" })}
                      </span>
                      <span className={groupRoleBadgeClass(normalizedGroupRole)} data-role-badge>
                        {groupRoleLabel}
                      </span>
                    </div>
                  </div>
                </div>

                {isNativeActor ? (
                  <div className="space-y-2 text-xs text-[var(--color-text-muted)]">
                    <span className="font-medium">
                      {t(sourceDirectory ? "workingDirectory" : "runtimeSwitch.ccccWorkspace")}
                    </span>
                    <div
                      className="break-all rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2.5 text-sm text-[var(--color-text-secondary)]"
                      aria-readonly="true"
                    >
                      {sourceDirectory ||
                        scopes.find(
                          (scope) => scope.scope_key === (savedDefaultScopeKey || activeScopeKey),
                        )?.url ||
                        t("runtimeSwitch.directoryUnavailable")}
                    </div>
                    <p>{t("nativeSession.configHint")}</p>
                  </div>
                ) : attachedScopes.length > 0 || defaultScopeKey || groupId ? (
                  <ActorWorkingDirectory
                    key={`${groupId}/${actorId}/${isOpen}`}
                    groupId={groupId}
                    scopes={attachedScopes}
                    activeScopeKey={activeScopeKey}
                    value={defaultScopeKey}
                    onChange={setDefaultScopeKey}
                    onScopeAttached={onScopeAttached}
                    onBusyChange={setDirectoryBusy}
                    disabled={busy === "actor-update"}
                    hint={
                      defaultScopeKey !== savedDefaultScopeKey
                        ? t("workingDirectoryEditHint")
                        : undefined
                    }
                  />
                ) : null}

                <div>
                  <RolePresetPicker
                    showDescription={false}
                    draftValue={actorNotes}
                    onChangeDraft={onChangeActorNotes}
                    disabled={actorNotesBusy || actorNotesError || busy === "actor-update"}
                  />
                  <label className="block text-xs font-medium mt-3 mb-2 text-[var(--color-text-muted)]">
                    {t("actorNotes")}
                  </label>
                  <Textarea
                    className="min-h-[144px]"
                    value={actorNotes}
                    onChange={(e) => onChangeActorNotes(e.target.value)}
                    placeholder={t("actorNotesPlaceholder")}
                    spellCheck={false}
                    disabled={actorNotesBusy || actorNotesError}
                  />
                  {actorNotesBusy ? (
                    <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">
                      {t("loadingActorNotes")}
                    </div>
                  ) : null}
                  {actorNotesError ? (
                    <div role="alert" className="mt-2 text-xs text-rose-600 dark:text-rose-400">
                      {t("actorNotesLoadFailed")}{" "}
                      <Button type="button" variant="outline" size="sm" onClick={onRetryActorNotes}>
                        {t("common:retry")}
                      </Button>
                    </div>
                  ) : null}
                </div>
              </div>
            </Surface>

            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionRuntime", "Runtime & Profile")}</div>
              {category !== currentCategory && category !== "native" ? (
                <p className="mt-1 text-xs text-[var(--color-text-muted)]">
                  {t("sectionRuntimeHint")}
                </p>
              ) : null}

              <div className="mt-4">
                <RuntimeCategoryPicker
                  value={category}
                  current={currentCategory}
                  canSwitch={canSwitch}
                  disabled={busy === "actor-update" || switchPending || managedSwitchBlocked}
                  onChange={changeCategory}
                />
              </div>
              {nativeSwitchVisited && canSwitch ? (
                <div className="mt-4" hidden={category !== "native"}>
                  {renderSwitchControls("native")}
                </div>
              ) : null}
              {isNativeActor && managedSwitchVisited && canSwitch ? (
                <div className="mt-4" hidden={category !== "managed"}>
                  {renderSwitchControls("managed")}
                </div>
              ) : null}
              {isNativeActor && webSwitchVisited && canSwitch ? (
                <div className="mt-4" hidden={category !== "web"}>
                  {renderSwitchControls("web")}
                </div>
              ) : null}
              {!isNativeActor && category !== "native" ? (
                <>
                  <div className="mt-4">
                    <RuntimeConfigurationModePicker
                      value={editMode}
                      disabled={busy === "actor-update"}
                      onChange={(mode) => {
                        if (mode !== editMode && secretEditorOpen) {
                          setSecretsError(t("secretEditorIncomplete"));
                          return;
                        }
                        setEditMode(mode);
                        if (mode !== "profile") return;
                        setPendingConvertToCustom(false);
                        setLocalNotice("");
                        if (!actorProfilesBusy && selectableActorProfiles.length <= 0) {
                          void onRequestActorProfiles?.();
                        }
                      }}
                    />
                  </div>

                  <div className="mt-4 space-y-4">
                    {editMode === "profile" ? (
                      <RuntimeProfilePicker
                        value={attachProfileId}
                        profiles={selectableActorProfiles}
                        busy={actorProfilesBusy}
                        disabled={busy === "actor-update"}
                        emptyHint={
                          actorProfiles.length > 0
                            ? "ChatGPT Web Model is managed directly in Settings > ChatGPT Web Model."
                            : undefined
                        }
                        onChange={setAttachProfileId}
                      />
                    ) : effectiveLinked ? (
                      <Surface
                        className="border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3 text-[var(--color-text-primary)]"
                        radius="md"
                        padding="none"
                      >
                        <div className="text-sm font-medium">
                          {selectedProfileName
                            ? t("managedByProfileName", { name: selectedProfileName })
                            : t("managedByProfile")}
                        </div>
                        <div className="mt-1 text-xs text-[var(--color-text-secondary)]">
                          {t("managedByProfileCustomHint")}
                        </div>
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          className="mt-3 transition-all ease-spring duration-300"
                          onClick={convertToCustomDraft}
                          disabled={busy === "actor-update"}
                        >
                          {t("convertToCustom")}
                        </Button>
                      </Surface>
                    ) : (
                      <div className="space-y-4">
                        <div>
                          <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                            {t("runtime")}
                          </label>
                          <RuntimeSelector
                            showHint={false}
                            className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                            value={runtime}
                            runtimeMode={runtimeMode}
                            disabled={busy === "actor-update"}
                            allowedRuntimes={SUPPORTED_RUNTIMES.filter(
                              (item) => isWebModelRuntime(item) === (category === "web"),
                            )}
                            onChange={(value) => {
                              const next = value as SupportedRuntime;
                              onChangeRuntime(next);
                              const nextInfo = runtimes.find((r) => r.name === next);
                              const nextDefault = String(
                                nextInfo?.recommended_command || "",
                              ).trim();
                              onChangeCommand(nextDefault);
                            }}
                            ariaLabel={t("runtime")}
                          />
                          <OpenCodeManagedModelHint runtime={runtime} />
                          {supportsAcpMode(runtime) && (
                            <AcpRuntimeMode
                              runtime={runtime}
                              value={runtimeMode}
                              onChange={setRuntimeMode}
                              disabled={busy === "actor-update"}
                            />
                          )}
                        </div>

                        {isWebModelRuntime(runtime) ? (
                          <div
                            className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/20 bg-sky-500/5 text-sky-700 dark:text-sky-300"
                            data-status-tone="info"
                          >
                            <div className="font-medium">
                              {runtime === "grok_web_model"
                                ? "Grok Bot Web Model"
                                : runtime === "chatgpt_dots"
                                  ? "ChatGPT Dots"
                                  : t("webModelActorBoundConnectorTitle", {
                                      defaultValue: "ChatGPT Web Model",
                                    })}
                            </div>
                            <div className="mt-1">
                              {runtime === "grok_web_model"
                                ? t("settings:grokActor.hint")
                                : runtime === "chatgpt_dots"
                                  ? t("settings:dotsActor.createHint")
                                  : t("webModelActorBoundConnectorHint", {
                                      defaultValue:
                                        "Use shared login and one connector in global Web Model settings. After saving this Actor, pair its own conversation below.",
                                    })}
                            </div>
                          </div>
                        ) : null}

                        {runtime === "deepseek" ? (
                          <DeepSeekConnectionFields
                            keys={secretKeys}
                            changes={secretChanges}
                            onChange={setSecretChanges}
                            disabled={busy === "actor-update" || secretsBusy}
                            loading={secretsRefreshing}
                            loadFailed={secretKeysLoadFailed}
                            onSave={() => void submit(false)}
                            onSaveAndRestart={() => void submit(true)}
                            saveDisabled={saveDisabled || switchPending || managedSwitchBlocked}
                          />
                        ) : null}
                        <DeepSeekCommandDetails runtime={runtime}>
                          {!isWebModelRuntime(runtime) && (
                            <div>
                              <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                                {t("command")}
                              </label>
                              <Input
                                className="font-mono"
                                value={command}
                                onChange={(e) => onChangeCommand(e.target.value)}
                                placeholder={defaultCommand || t("enterCommand")}
                              />
                              {isRunning ? (
                                <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                                  {t("runtimeChangesNote")}
                                </div>
                              ) : null}
                              {defaultCommand.trim() ? (
                                <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                                  {t("default")}{" "}
                                  <code className="px-1 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
                                    {defaultCommand}
                                  </code>
                                </div>
                              ) : null}
                            </div>
                          )}
                        </DeepSeekCommandDetails>
                      </div>
                    )}
                    {stagingGrok && (
                      <div className="space-y-2">
                        <label className="block text-sm" htmlFor="edit-grok-bot-url">
                          {t("settings:grokActor.url")}
                        </label>
                        <Input
                          id="edit-grok-bot-url"
                          type="url"
                          required
                          value={grokBotUrl ?? ""}
                          onChange={(e) => setGrokBotUrl(e.target.value)}
                          disabled={busy === "actor-update" || secretsBusy}
                          placeholder="https://grok.com/bot/…"
                          aria-describedby="edit-grok-bot-url-hint"
                        />
                        <p
                          id="edit-grok-bot-url-hint"
                          className="text-xs text-[var(--color-text-muted)]"
                        >
                          {t("settings:grokActor.runtimeSaveHint")}
                        </p>
                        <Button type="button" variant="outline" onClick={openSharedSettings}>
                          {t("settings:grokActor.shared")}
                        </Button>
                      </div>
                    )}
                  </div>
                </>
              ) : null}
            </Surface>
          </div>

          {isWebModelRuntime(conversationRuntime) && conversationRuntime === savedRuntime && (
            <div
              ref={conversationRef}
              tabIndex={-1}
              className="scroll-mt-4 outline-none"
              aria-label={t(
                conversationRuntime === "grok_web_model"
                  ? "settings:grokActor.title"
                  : conversationRuntime === "chatgpt_dots"
                    ? "settings:dotsActor.title"
                    : "settings:webModelActor.title",
              )}
            >
              <p className="mb-3 text-xs text-[var(--color-text-muted)]">
                {t("settings:webModelActor.immediateHint")}
              </p>
              {conversationRuntime === "chatgpt_dots" ? (
                <DotsActorPanel
                  key={`${groupId}/${actorId}/dots`}
                  groupId={groupId}
                  actorId={actorId}
                  isVisible={!suspended}
                  isRunning={isRunning}
                  canAccessConnection={canAccessGlobalSettings}
                  setup
                  onOpenSharedSettings={openSharedSettings}
                  onBusyChange={setConnectionBusy}
                  saving={secretsBusy || busy === "actor-update"}
                />
              ) : conversationRuntime === "grok_web_model" ? (
                <GrokActorSetup
                  key={`${groupId}/${actorId}/grok`}
                  canAccessConnection={canAccessGlobalSettings}
                  groupId={groupId}
                  actorId={actorId}
                  isDark={isDark}
                  isVisible={!suspended}
                  onOpenSharedSettings={openSharedSettings}
                  draftUrl={grokBotUrl}
                  onDraftChange={setGrokBotUrl}
                  onEnabledChange={setConnectionEnabled}
                  onUnresolvedChange={setConnectionUnresolved}
                  onBusyChange={setConnectionBusy}
                  saving={secretsBusy || busy === "actor-update"}
                />
              ) : (
                <WebModelActorSetup
                  key={`${groupId}/${actorId}`}
                  groupId={groupId}
                  actorId={actorId}
                  isDark={isDark}
                  isVisible={!suspended}
                  canAccessConnection={canAccessGlobalSettings}
                  onOpenSharedSettings={openSharedSettings}
                  onBusyChange={setConnectionBusy}
                  saving={secretsBusy || busy === "actor-update"}
                />
              )}
            </div>
          )}

          {!isNativeActor && category !== "native" ? (
            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionAdvanced", "Advanced")}</div>
              <div className="mt-4">
                <ActorConfigTabs
                  ariaLabel={t("sectionAdvanced", "Advanced")}
                  activeId={activeAdvancedTab}
                  onChange={selectAdvancedTab}
                  tabs={[
                    ...(showRuntimeSetup
                      ? [
                          {
                            id: "connection",
                            label: t("runtimeSetupSection", "Connection setup"),
                            panel: (
                              <div
                                className="rounded-xl border px-3 py-2 text-[11px] border-amber-500/20 bg-amber-500/5 text-amber-700 dark:text-amber-400"
                                data-status-tone="warning"
                              >
                                <div className="font-medium">{t("manualMcpRequired")}</div>
                                <div className="mt-1">
                                  {t("configureMcpStdio")}{" "}
                                  <code className="px-1 rounded bg-amber-500/15">cccc</code>{" "}
                                  {t("thatRuns")}{" "}
                                  <code className="px-1 rounded bg-amber-500/15">cccc mcp</code>.
                                </div>
                                <pre className="mt-1.5 p-2 rounded overflow-x-auto whitespace-pre bg-amber-500/10 text-amber-800 dark:text-amber-100">
                                  <code>{BASIC_MCP_CONFIG_SNIPPET}</code>
                                </pre>
                                <div className="mt-1 text-[10px] text-amber-700/80 dark:text-amber-400/80">
                                  {t("restartAfterConfig")}
                                </div>
                              </div>
                            ),
                          },
                        ]
                      : []),
                    ...(editMode === "custom" && !isWebModelRuntime(runtime)
                      ? [
                          {
                            id: "environment",
                            label: t("secretsSection", "Environment variables"),
                            panel: (
                              <>
                                {linked &&
                                pendingConvertToCustom &&
                                currentCategory === "managed" ? (
                                  <p className="mb-3 text-xs text-[var(--color-text-muted)]">
                                    {t("runtimeSwitch.profileToCustomSecrets")}
                                  </p>
                                ) : null}
                                <ActorSecretManager
                                  keys={secretKeys}
                                  masks={secretMasks}
                                  changes={secretChanges}
                                  loading={secretsRefreshing}
                                  keysLoadFailed={secretKeysLoadFailed}
                                  disabled={
                                    secretsBusy || secretsRefreshing || busy === "actor-update"
                                  }
                                  onRefresh={() => void refreshSecretKeys()}
                                  onChangesChange={setSecretChanges}
                                  onDraftOpenChange={setSecretEditorOpen}
                                />
                              </>
                            ),
                          },
                        ]
                      : []),
                    {
                      id: "capabilities",
                      label: t("capabilitiesSection", "Capabilities"),
                      panel: (
                        <CapabilityPicker
                          isDark={isDark}
                          value={parseCapabilityIdInput(capabilityAutoloadText)}
                          onChange={(next) =>
                            onChangeCapabilityAutoloadText(formatCapabilityIdInput(next))
                          }
                          disabled={busy === "actor-update"}
                          active={capabilitiesPrimed || activeAdvancedTab === "capabilities"}
                          label={t("autoloadCapabilities")}
                          hint={t("autoloadCapabilitiesHint")}
                        />
                      ),
                    },
                    ...(editMode === "custom"
                      ? [
                          {
                            id: "profile",
                            label: t("profileToolsSection", "Profile tools"),
                            panel: (
                              <div className="flex flex-wrap gap-3">
                                {isWebModelRuntime(runtime) && (
                                  <p className="w-full text-sm text-[var(--color-text-secondary)]">
                                    {t("webModelProfileHint")}
                                  </p>
                                )}
                                <Button
                                  type="button"
                                  variant="secondary"
                                  onClick={() => void saveAsProfile()}
                                  disabled={
                                    busy === "actor-profile-save" || busy === "actor-update"
                                  }
                                >
                                  {busy === "actor-profile-save"
                                    ? t("savingProfile")
                                    : t("addToActorProfiles")}
                                </Button>
                              </div>
                            ),
                          },
                        ]
                      : []),
                  ]}
                />
              </div>
            </Surface>
          ) : null}
        </fieldset>
      </div>
      {managedSwitchPayload && savedActor
        ? createPortal(
            <RuntimeSwitchConfirm
              isDark={isDark}
              actor={savedActor}
              targetLabel={
                managedSwitchPayload.mode === "profile"
                  ? selectedProfile?.name?.trim() || selectedProfile?.id || runtime
                  : runtime
              }
              targetDirectory={
                isWebModelRuntime(conversationRuntime)
                  ? undefined
                  : attachedScopes.find(
                      (scope) => scope.scope_key === (defaultScopeKey || activeScopeKey),
                    )?.url
              }
              targetKind={isWebModelRuntime(conversationRuntime) ? "web" : "managed"}
              noStart={managedSwitchNoStart}
              onNoStart={setManagedSwitchNoStart}
              busy={secretsBusy || managedProfileBusy || busy === "actor-update"}
              confirmDisabled={managedProfileSaveFailed}
              message={managedProfileMessage}
              onSaveProfile={
                !linked &&
                !isWebModelRuntime(savedRuntime) &&
                isWebModelRuntime(conversationRuntime)
                  ? async () => {
                      setManagedProfileBusy(true);
                      try {
                        const saved = await onSaveAsProfile(
                          undefined,
                          savedActor.runtime_mode || "default",
                          { savedActor: true },
                        );
                        const ok = Boolean(saved?.profileId);
                        setManagedProfileSaveFailed(!ok);
                        setManagedProfileMessage(
                          t(ok ? "runtimeSwitch.profileSaved" : "runtimeSwitch.profileSaveFailed"),
                        );
                      } catch {
                        setManagedProfileSaveFailed(true);
                        setManagedProfileMessage(t("runtimeSwitch.profileSaveFailed"));
                      } finally {
                        setManagedProfileBusy(false);
                      }
                    }
                  : undefined
              }
              profileToCustom={linked && managedSwitchPayload.mode === "custom"}
              keepCustomSecrets={
                !linked &&
                managedSwitchPayload.mode === "custom" &&
                !isWebModelRuntime(conversationRuntime) &&
                secretKeys.length > 0
              }
              secretKeys={secretKeys}
              onCancel={() => setManagedSwitchPayload(null)}
              onConfirm={() => {
                setSecretsBusy(true);
                setSecretsError("");
                void onSaveAndRestart({
                  ...managedSwitchPayload,
                  runtimeSwitch: { start: !managedSwitchNoStart },
                })
                  .then(() => setManagedSwitchPayload(null))
                  .catch((error: unknown) => {
                    const message = error instanceof Error ? error.message : t("saveFailed");
                    if (
                      message.startsWith("actor_switch_unknown:") ||
                      message.startsWith("actor_switch_conflict:")
                    )
                      setManagedSwitchBlocked(true);
                    setManagedSwitchPayload(null);
                    setSecretsError(message.replace(/^actor_switch_[a-z_]+:\s*/, ""));
                  })
                  .finally(() => setSecretsBusy(false));
              }}
            />,
            document.body,
          )
        : null}
    </ModalFrame>
  );
}
