import { ActorProfile, RuntimeInfo, SupportedRuntime, SUPPORTED_RUNTIMES, RUNTIME_INFO } from "../../types";
import { useTranslation } from "react-i18next";
import { BASIC_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { parsePrivateEnvSetText, parsePrivateEnvUnsetText } from "../../utils/privateEnvInput";
import { formatCapabilityIdInput, parseCapabilityIdInput } from "../../utils/capabilityAutoload";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { CapabilityPicker } from "../CapabilityPicker";
import { RolePresetPicker } from "../RolePresetPicker";
import { ActorAvatarField } from "../ActorAvatarField";
import { SelectCombobox } from "../SelectCombobox";
import { normalizeActorRunner, supportsStandardWebHeadlessRuntime } from "../../utils/headlessRuntimeSupport";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";
import { Textarea } from "../ui/textarea";
import { ModalFrame } from "./ModalFrame";

type ConfigMode = "custom" | "profile";

export interface EditActorSavePayload {
  mode: ConfigMode;
  runner: "pty" | "headless";
  setVars: Record<string, string>;
  unsetKeys: string[];
  clear: boolean;
  capabilityAutoload: string[];
  profileId?: string;
  convertToCustom?: boolean;
}

export interface SaveActorProfileResult {
  profileId?: string;
  profileName?: string;
  useNow?: boolean;
}

export const NO_CHANGES_SENTINEL = "CCCC_NO_CHANGES";

interface ActorConfigBaseProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  runtimes: RuntimeInfo[];
  actorProfiles: ActorProfile[];
  actorProfilesBusy: boolean;
  onRequestActorProfiles?: () => Promise<void> | void;
  onSaveAsProfile: () => Promise<SaveActorProfileResult | void> | void;
  onCancel: () => void;
}

export interface EditActorConfigProps extends ActorConfigBaseProps {
  mode: "edit";
  groupId: string;
  actorId: string;
  avatarUrl?: string | null;
  hasCustomAvatar?: boolean;
  isRunning: boolean;
  runtime: SupportedRuntime;
  onChangeRuntime: (runtime: SupportedRuntime) => void;
  runner: "pty" | "headless";
  onChangeRunner: (runner: "pty" | "headless") => void;
  command: string;
  onChangeCommand: (command: string) => void;
  title: string;
  onChangeTitle: (title: string) => void;
  roleNotes: string;
  onChangeRoleNotes: (value: string) => void;
  roleNotesBusy?: boolean;
  capabilityAutoloadText: string;
  onChangeCapabilityAutoloadText: (value: string) => void;
  onSave: (payload: EditActorSavePayload) => Promise<void>;
  onSaveAndRestart: (payload: EditActorSavePayload) => Promise<void>;
  linkedProfileId?: string;
  linkedProfileScope?: "global" | "user";
  linkedProfileOwner?: string;
  onAvatarChanged?: () => Promise<void>;
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
  runner: "pty" | "headless";
  onChangeRunner: (runner: "pty" | "headless") => void;
  command: string;
  onChangeCommand: (command: string) => void;
  useDefaultCommand: boolean;
  onChangeUseDefaultCommand: (value: boolean) => void;
  secretsSetText: string;
  onChangeSecretsSetText: (value: string) => void;
  capabilityAutoloadText: string;
  onChangeCapabilityAutoloadText: (value: string) => void;
  roleNotes: string;
  onChangeRoleNotes: (value: string) => void;
  error: string;
  onChangeError: (message: string) => void;
  canSubmit: boolean;
  submitDisabledReason: string;
  onCreate: (avatarFile?: File | null) => Promise<boolean> | boolean;
}

export type ActorConfigModalProps = EditActorConfigProps | CreateActorConfigProps;

type SecretSource = "none" | "actor" | "profile-preview";

/** Runtime-specific placeholder hints for secret environment variables */
const SECRETS_PLACEHOLDER: Record<string, { set: string; unset: string }> = {
  claude: {
    set: 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."',
    unset: "ANTHROPIC_AUTH_TOKEN\nANTHROPIC_BASE_URL",
  },
  codex: {
    set: "# Configure OpenAI-compatible Codex providers with Codex config or command -c overrides.",
    unset: "",
  },
  gemini: {
    set: 'GOOGLE_API_KEY="..."',
    unset: "GOOGLE_API_KEY",
  },
  hermes: {
    set: "# Configure Hermes providers, OAuth, and tools in your Hermes profile.",
    unset: "",
  },
};

const DEFAULT_SECRETS_PLACEHOLDER = {
  set: 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."',
  unset: "ANTHROPIC_AUTH_TOKEN\nANTHROPIC_BASE_URL",
};

function commandPreview(command: string[] | undefined): string {
  const cmd = Array.isArray(command) ? command.filter((item) => typeof item === "string" && item.trim()) : [];
  return cmd.join(" ");
}

function profileScopeLabel(profile: ActorProfile, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (String(profile.scope || "global").trim() === "user") {
    return t("profileScopeOwnedBy", { owner: String(profile.owner_id || "").trim() || "?" });
  }
  return t("profileScopeGlobal");
}

function isWebModelProfile(profile: ActorProfile): boolean {
  return String(profile.runtime || "").trim().toLowerCase() === "web_model";
}

function modeButtonClass(selected: boolean): string {
  return [
    "px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-all ease-spring duration-300",
    selected
      ? "border-[var(--color-text-primary)] bg-[var(--color-text-primary)] text-[var(--color-bg-primary)] hover:bg-[var(--color-text-primary)] hover:text-[var(--color-bg-primary)] hover:opacity-90"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
  ].join(" ");
}

function CreateActorConfigModal({
  isOpen,
  isDark,
  busy,
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
  runner,
  onChangeRunner,
  command,
  onChangeCommand,
  useDefaultCommand,
  onChangeUseDefaultCommand,
  secretsSetText,
  onChangeSecretsSetText,
  capabilityAutoloadText,
  onChangeCapabilityAutoloadText,
  roleNotes,
  onChangeRoleNotes,
  error,
  onChangeError,
  canSubmit,
  submitDisabledReason,
  onCreate,
  onSaveAsProfile,
  onCancel,
}: CreateActorConfigProps) {
  const { t } = useTranslation("actors");
  const { modalRef } = useModalA11y(isOpen, onCancel);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [capabilitiesPrimed, setCapabilitiesPrimed] = useState(false);
  const avatarPreviewUrl = useMemo(() => (avatarFile ? URL.createObjectURL(avatarFile) : null), [avatarFile]);

  useEffect(() => {
    return () => {
      if (avatarPreviewUrl) URL.revokeObjectURL(avatarPreviewUrl);
    };
  }, [avatarPreviewUrl]);

  const selectableActorProfiles = useMemo(
    () => actorProfiles.filter((profile) => !isWebModelProfile(profile)),
    [actorProfiles],
  );

  useEffect(() => {
    if (!isOpen || !useProfile || !String(profileId || "").trim()) return;
    const selected = selectableActorProfiles.some((profile) => actorProfileIdentityKey(profile) === String(profileId || "").trim());
    if (!selected) onChangeProfileId("");
  }, [isOpen, onChangeProfileId, profileId, selectableActorProfiles, useProfile]);

  if (!isOpen) return null;

  const selectedProfile = selectableActorProfiles.find((item) => actorProfileIdentityKey(item) === String(profileId || "").trim());
  const selectedProfileRuntime = String(selectedProfile?.runtime || "").trim() as SupportedRuntime;
  const selectedProfileCommand = commandPreview(selectedProfile?.command);
  const selectedProfileRunner = normalizeActorRunner(selectedProfile?.runner || runner);
  const runtimeInfo = runtimes.find((r) => r.name === runtime);
  const runtimeAvailable = runtimeInfo?.available ?? false;
  const defaultCommand = runtimeInfo?.recommended_command || "";
  const previewRuntime = useProfile ? selectedProfileRuntime || null : runtime;
  const previewTitle = String(actorId || "").trim() || suggestedActorId;
  const customRunnerLockedToPty = !useProfile && !supportsStandardWebHeadlessRuntime(runtime);
  const webModelRunnerLockedToHeadless = !useProfile && runtime === "web_model";
  const showRuntimeSetup = !useProfile && runtime === "custom";
  const showCommandEditor = !useProfile && (runtime === "custom" || !useDefaultCommand);
  const webModelSetupIsActorBound = !useProfile && runtime === "web_model";
  const secretsPlaceholder = (SECRETS_PLACEHOLDER[runtime] ?? DEFAULT_SECRETS_PLACEHOLDER).set;
  const sectionCardClass = "rounded-2xl p-4 sm:p-5 glass-panel";
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const sectionHintClass = "mt-1 text-xs text-[var(--color-text-muted)]";
  const collapsibleSummaryClass = "flex cursor-pointer list-none items-start justify-between gap-3 [&::-webkit-details-marker]:hidden";
  const collapsibleLabelClass = "text-xs font-medium text-[var(--color-text-secondary)]";
  const collapsibleChevronClass = "text-sm transition-transform group-open:rotate-180 text-[var(--color-text-tertiary)]";
  const nestedCardClass = "group rounded-xl border p-3 border-[var(--glass-border-subtle)] bg-[var(--glass-bg)] transition-all duration-300 hover:border-[var(--glass-border)]";

  const handleSubmit = async () => {
    try {
      const ok = await Promise.resolve(onCreate(avatarFile));
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
      isOpen={isOpen}
      isDark={isDark}
      onClose={handleCancel}
      titleId="actor-config-create-title"
      title={
        <div>
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">{t("addAiAgent")}</div>
          <div className="text-sm mt-1 text-[var(--color-text-muted)]">{t("addActorSubtitle")}</div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="w-full h-full sm:h-auto sm:w-[min(100vw-2rem,72rem)] sm:max-w-[72rem] sm:mt-6 sm:max-h-[calc(100vh-5rem)]"
      modalRef={modalRef}
      footerActions={
        <>
          {error ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-rose-500/20 bg-rose-500/5 text-rose-600 dark:text-rose-400"
              role="alert"
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

          <div className="flex flex-col-reverse sm:flex-row gap-3 sm:justify-end">
            <Button type="button" variant="secondary" className="w-full sm:w-auto" onClick={handleCancel}>
              {t("common:cancel")}
            </Button>
            <div className="w-full sm:w-auto sm:min-w-[14rem]">
              <Button
                type="button"
                className="w-full font-semibold"
                onClick={() => void handleSubmit()}
                disabled={!canSubmit}
              >
                {busy === "actor-add" ? t("adding") : useProfile ? t("createFromProfile") : t("addAgent")}
              </Button>
              {submitDisabledReason ? (
                <div className="text-[10px] text-amber-700 dark:text-amber-400 mt-1.5">{submitDisabledReason}</div>
              ) : null}
            </div>
          </div>
        </>
      }
    >
      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-hide bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_30%),linear-gradient(180deg,rgb(251,250,247),rgb(245,244,241))] p-4 dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))] sm:p-6">
        <div className="mx-auto w-full max-w-6xl space-y-4">
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1.08fr)_minmax(22rem,0.92fr)] xl:items-start">
            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionBasics")}</div>
              <div className={sectionHintClass}>{t("addSectionBasicsHint")}</div>

              <div className="mt-4 space-y-4">
                <div className="grid gap-4 sm:grid-cols-[88px_minmax(0,1fr)] sm:items-start">
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
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("agentId", { defaultValue: "Agent ID" })}</label>
                      <Input value={actorId} onChange={(e) => onChangeActorId(e.target.value)} placeholder={suggestedActorId} disabled={busy === "actor-add"} />
                      <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                        {t("leaveEmptyToUse")} <code className="px-1 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{suggestedActorId}</code>
                      </div>
                    </div>

                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("agentRole", { defaultValue: "Agent role" })}</label>
                      <div className="grid grid-cols-2 gap-2">
                        <Button type="button" variant="outline" className={modeButtonClass(role === "peer")} onClick={() => onChangeRole("peer")} disabled={busy === "actor-add"}>
                          {t("peerRole")}
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          className={modeButtonClass(role === "foreman")}
                          onClick={() => onChangeRole("foreman")}
                          disabled={busy === "actor-add" || hasForeman}
                          title={hasForeman ? t("foremanAlreadyConfigured", { defaultValue: "Foreman already configured" }) : undefined}
                        >
                          {t("foremanRole")}
                        </Button>
                      </div>
                      <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{hasForeman ? t("foremanLeads") : t("firstAgentForeman")}</div>
                    </div>
                  </div>
                </div>

                <div>
                  <RolePresetPicker draftValue={roleNotes} onChangeDraft={onChangeRoleNotes} disabled={busy === "actor-add"} />
                  <label className="block text-xs font-medium mt-3 mb-2 text-[var(--color-text-muted)]">{t("roleNotes")}</label>
                  <Textarea
                    className="min-h-[144px]"
                    value={roleNotes}
                    onChange={(e) => onChangeRoleNotes(e.target.value)}
                    placeholder={t("roleNotesPlaceholder")}
                    spellCheck={false}
                    disabled={busy === "actor-add"}
                  />
                  <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("newActorRoleNotesHint")}</div>
                </div>
              </div>
            </Surface>

            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionRuntime", "Runtime & Profile")}</div>
              <div className={sectionHintClass}>{t("sectionRuntimeHint")}</div>

              <div className="mt-4">
                <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("creationMode")}</label>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  <Button
                    type="button"
                    variant="outline"
                    className={modeButtonClass(!useProfile)}
                    onClick={() => onChangeUseProfile(false)}
                    disabled={busy === "actor-add"}
                  >
                    {t("customAgent")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    className={modeButtonClass(useProfile)}
                    onClick={() => {
                      onChangeUseProfile(true);
                      if (!actorProfilesBusy && selectableActorProfiles.length <= 0) void onRequestActorProfiles?.();
                      if (selectableActorProfiles.length > 0 && !profileId) onChangeProfileId(actorProfileIdentityKey(selectableActorProfiles[0]));
                    }}
                    disabled={busy === "actor-add"}
                  >
                    {t("fromActorProfile")}
                  </Button>
                </div>
              </div>

              <div className="mt-4 space-y-4">
                {useProfile ? (
                  <>
                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("actorProfile")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-3 py-2 text-sm min-h-[40px] glass-input text-[var(--color-text-primary)]"
                        value={profileId}
                        onChange={onChangeProfileId}
                        disabled={actorProfilesBusy || busy === "actor-add"}
                        ariaLabel={t("actorProfile")}
                        items={[
                          { value: "", label: actorProfilesBusy ? t("loadingProfiles") : t("selectActorProfile") },
                          ...selectableActorProfiles.map((profile) => ({
                            value: actorProfileIdentityKey(profile),
                            label: `${profile.name || profile.id} · ${profileScopeLabel(profile, t)}`,
                          })),
                        ]}
                        searchable
                      />
                    </div>
                    {selectedProfile ? (
                      <Surface className="px-3 py-2 text-xs text-[var(--color-text-secondary)]" variant="subtle" radius="md" padding="none">
                        <div className="font-medium">{selectedProfile.name || selectedProfile.id}</div>
                        <div className="mt-1">{profileScopeLabel(selectedProfile, t)}</div>
                        <div className="mt-1 flex flex-wrap items-center gap-2">
                          <span>{RUNTIME_INFO[selectedProfileRuntime]?.label || selectedProfileRuntime}</span>
                          <span className="rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                            {selectedProfileRunner === "headless" ? t("headless") : t("pty", { defaultValue: "PTY" })}
                          </span>
                        </div>
                        {selectedProfileCommand ? <div className="mt-1 font-mono break-all">{selectedProfileCommand}</div> : null}
                      </Surface>
                    ) : null}
                  </>
                ) : (
                  <div className="space-y-4">
                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("runtime")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                        value={runtime}
                        onChange={(value) => {
                          const next = value as SupportedRuntime;
                          onChangeRuntime(next);
                          if (next === "custom") onChangeUseDefaultCommand(false);
                          else onChangeUseDefaultCommand(true);
                          if (next === "web_model") onChangeRunner("headless");
                          else if (!supportsStandardWebHeadlessRuntime(next)) onChangeRunner("pty");
                          const nextInfo = runtimes.find((r) => r.name === next);
                          onChangeCommand(String(nextInfo?.recommended_command || "").trim());
                        }}
                        ariaLabel={t("runtime")}
                        items={SUPPORTED_RUNTIMES.map((rt) => {
                          const info = RUNTIME_INFO[rt];
                          const rtInfoLocal = runtimes.find((r) => r.name === rt);
                          const available = rtInfoLocal?.available ?? false;
                          return {
                            value: rt,
                            label: `${info?.label || rt}${!available && rt !== "custom" ? ` ${t("notInstalled")}` : ""}`,
                            disabled: !available && rt !== "custom",
                          };
                        })}
                        searchable
                      />
                    </div>

                    {runtime ? (
                      <Surface className="px-3 py-2 text-xs text-[var(--color-text-secondary)]" variant="subtle" radius="md" padding="none">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <span>{runtimeAvailable ? t("available") : t("notAvailable", { defaultValue: "Not available" })}</span>
                          <span>{runtime === "custom" ? t("custom") : defaultCommand || "—"}</span>
                        </div>
                      </Surface>
                    ) : null}

                    {supportsStandardWebHeadlessRuntime(runtime) ? (
                      <div>
                        <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("runnerMode")}</label>
                        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                          <Button type="button" variant="outline" className={modeButtonClass(runner === "pty")} onClick={() => onChangeRunner("pty")} disabled={webModelRunnerLockedToHeadless}>
                            {t("pty", { defaultValue: "PTY" })}
                          </Button>
                          <Button type="button" variant="outline" className={modeButtonClass(runner === "headless")} onClick={() => onChangeRunner("headless")} disabled={customRunnerLockedToPty}>
                            {t("headless")}
                          </Button>
                        </div>
                      </div>
                    ) : null}

                    {runtime !== "custom" && runtime !== "web_model" ? (
                      <label className="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
                        <input
                          type="checkbox"
                          checked={useDefaultCommand}
                          onChange={(event) => onChangeUseDefaultCommand(event.target.checked)}
                          disabled={busy === "actor-add"}
                        />
                        {t("useRuntimeDefaultCommand")}
                      </label>
                    ) : null}

                    {showCommandEditor ? (
                      <div>
                        <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{runtime === "custom" ? t("command") : t("customCommandOverride")}</label>
                        <Input
                          className="font-mono"
                          value={command}
                          onChange={(e) => onChangeCommand(e.target.value)}
                          placeholder={defaultCommand || "/path/to/custom-agent --option"}
                          disabled={busy === "actor-add"}
                        />
                      </div>
                    ) : null}

                    {webModelSetupIsActorBound ? (
                      <div className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/20 bg-sky-500/5 text-sky-700 dark:text-sky-300">
                        <div className="font-medium">{t("webModelActorBoundConnectorTitle", { defaultValue: "Single ChatGPT Web Model actor" })}</div>
                        <div className="mt-1">
                          {t("webModelActorBoundConnectorHint", {
                            defaultValue: "Manage the ChatGPT MCP URL and target conversation in Settings > ChatGPT Web Model. CCCC supports one ChatGPT Web Model actor in this instance.",
                          })}
                        </div>
                      </div>
                    ) : null}
                  </div>
                )}
              </div>
            </Surface>
          </div>

          <details className={`group ${sectionCardClass}`}>
            <summary className={collapsibleSummaryClass}>
              <div>
                <div className={sectionTitleClass}>{t("sectionAdvanced")}</div>
                <div className={sectionHintClass}>{t("sectionAdvancedHint")}</div>
              </div>
              <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
            </summary>

            <div className="mt-4 space-y-4 border-t border-[var(--glass-border-subtle)] pt-4">
              {showRuntimeSetup ? (
                <details className={nestedCardClass}>
                  <summary className={collapsibleSummaryClass}>
                    <div>
                      <div className={collapsibleLabelClass}>{t("runtimeSetupSection")}</div>
                      <div className={sectionHintClass}>{t("runtimeSetupSectionHint")}</div>
                    </div>
                    <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                  </summary>
                  <div className="mt-4 rounded-xl border px-3 py-2 text-[11px] border-amber-500/20 bg-amber-500/5 text-amber-700 dark:text-amber-400">
                    <div className="font-medium">{t("manualMcpRequired")}</div>
                    <div className="mt-1">
                      {t("configureMcpStdio")} <code className="px-1 rounded bg-amber-500/15">cccc</code> {t("thatRuns")} <code className="px-1 rounded bg-amber-500/15">cccc mcp</code>.
                    </div>
                    <pre className="mt-1.5 p-2 rounded overflow-x-auto whitespace-pre bg-amber-500/10 text-amber-800 dark:text-amber-100">
                      <code>{BASIC_MCP_CONFIG_SNIPPET}</code>
                    </pre>
                    <div className="mt-1 text-[10px] text-amber-700/80 dark:text-amber-400/80">{t("restartAfterConfig")}</div>
                  </div>
                </details>
              ) : null}

              {!useProfile ? (
                <details className={nestedCardClass}>
                  <summary className={collapsibleSummaryClass}>
                    <div>
                      <div className={collapsibleLabelClass}>{t("secretsSection")}</div>
                      <div className={sectionHintClass}>{t("secretsSectionHint")}</div>
                    </div>
                    <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                  </summary>
                  <div className="mt-4">
                    <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("secretsWriteOnly")}</label>
                    <Textarea
                      className="min-h-[96px] font-mono"
                      value={secretsSetText}
                      onChange={(e) => onChangeSecretsSetText(e.target.value)}
                      placeholder={secretsPlaceholder}
                      disabled={busy === "actor-add"}
                    />
                    <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("secretsStoredLocally").replace(/<1>|<\/1>/g, "")}</div>
                    <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">{t("secretsFormat").replace(/<1>|<\/1>|<2>|<\/2>/g, "")}</div>
                  </div>
                </details>
              ) : null}

              {previewRuntime ? (
                <details
                  className={nestedCardClass}
                  onToggle={(event) => {
                    const open = (event.currentTarget as HTMLDetailsElement).open;
                    if (!open || capabilitiesPrimed) return;
                    setCapabilitiesPrimed(true);
                  }}
                >
                  <summary className={collapsibleSummaryClass}>
                    <div>
                      <div className={collapsibleLabelClass}>{t("capabilitiesSection")}</div>
                      <div className={sectionHintClass}>{t("capabilitiesSectionHint")}</div>
                    </div>
                    <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                  </summary>
                  <div className="mt-4">
                    <CapabilityPicker
                      isDark={isDark}
                      value={parseCapabilityIdInput(capabilityAutoloadText)}
                      onChange={(next) => onChangeCapabilityAutoloadText(formatCapabilityIdInput(next))}
                      disabled={busy === "actor-add"}
                      active={capabilitiesPrimed}
                      label={t("autoloadCapabilities")}
                      hint={t("autoloadCapabilitiesHint")}
                    />
                  </div>
                </details>
              ) : null}

              {!useProfile && !webModelSetupIsActorBound ? (
                <details className={nestedCardClass}>
                  <summary className={collapsibleSummaryClass}>
                    <div>
                      <div className={collapsibleLabelClass}>{t("profileToolsSection")}</div>
                      <div className={sectionHintClass}>{t("profileToolsSectionHint")}</div>
                    </div>
                    <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                  </summary>
                  <div className="mt-4 flex flex-wrap gap-3">
                    <Button type="button" variant="secondary" onClick={() => void onSaveAsProfile()} disabled={busy === "actor-profile-save" || busy === "actor-add"}>
                      {busy === "actor-profile-save" ? t("savingProfile") : t("addToActorProfiles")}
                    </Button>
                  </div>
                </details>
              ) : null}
            </div>
          </details>
        </div>
      </div>
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
  isDark,
  busy,
  mode: _mode,
  groupId,
  actorId,
  avatarUrl,
  hasCustomAvatar = false,
  isRunning,
  runtimes,
  runtime,
  onChangeRuntime,
  runner,
  onChangeRunner,
  command,
  onChangeCommand,
  title,
  onChangeTitle,
  roleNotes,
  onChangeRoleNotes,
  roleNotesBusy = false,
  capabilityAutoloadText,
  onChangeCapabilityAutoloadText,
  onSave,
  onSaveAndRestart,
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
  const { modalRef } = useModalA11y(isOpen, onCancel);
  const [secretKeys, setSecretKeys] = useState<string[]>([]);
  const [secretMasks, setSecretMasks] = useState<Record<string, string>>({});
  const [secretsSetText, setSecretsSetText] = useState("");
  const [secretsUnsetText, setSecretsUnsetText] = useState("");
  const [secretsClearAll, setSecretsClearAll] = useState(false);
  const [secretsError, setSecretsError] = useState("");
  const [secretsBusy, setSecretsBusy] = useState(false);
  const [attachProfileId, setAttachProfileId] = useState("");
  const [editMode, setEditMode] = useState<ConfigMode>("custom");
  const [pendingConvertToCustom, setPendingConvertToCustom] = useState(false);
  const [localNotice, setLocalNotice] = useState("");
  const [secretSource, setSecretSource] = useState<SecretSource>("none");
  const [avatarBusy, setAvatarBusy] = useState<"" | "upload" | "clear">("");
  const [secretsPrimed, setSecretsPrimed] = useState(false);
  const [capabilitiesPrimed, setCapabilitiesPrimed] = useState(false);
  const secretFetchSeqRef = useRef(0);
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
    () => actorProfiles.filter((profile) => !isWebModelProfile(profile) || actorProfileIdentityKey(profile) === String(attachProfileId || "").trim()),
    [actorProfiles, attachProfileId]
  );
  const selectedProfile = useMemo(
    () => selectableActorProfiles.find((profile) => actorProfileIdentityKey(profile) === String(attachProfileId || "").trim()),
    [selectableActorProfiles, attachProfileId]
  );
  const selectedProfileName = String(selectedProfile?.name || "").trim();
  const selectedProfileRunner = normalizeActorRunner(selectedProfile?.runner || runner);

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
      const resp = await api.fetchActorProfilePrivateEnvKeys(profileId, {
        scope: linkedProfileScope,
        ownerId: linkedProfileOwner,
      });
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

      const mergedKeys = Array.isArray(resp.result?.keys) ? resp.result.keys : [];
      const mergedMasks =
        resp.result?.masked_values && typeof resp.result.masked_values === "object"
          ? resp.result.masked_values
          : {};

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
    setAttachProfileId(
      hasLinked
        ? actorProfileIdentityKey({
            id: String(linkedProfileId || "").trim(),
            scope: linkedProfileScope || "global",
            owner_id: linkedProfileOwner || "",
          })
        : ""
    );
    setLocalNotice("");
    setSecretsError("");
    setSecretMasks({});
    setSecretsSetText("");
    setSecretsUnsetText("");
    setSecretsClearAll(false);
    setSecretKeys([]);
    setSecretSource("none");
  }, [groupId, actorId, isOpen, linkedProfileId, linkedProfileOwner, linkedProfileScope]);

  useEffect(() => {
    if (!isOpen) return;
    if (editMode === "profile") {
      secretFetchSeqRef.current += 1;
      setSecretKeys([]);
      setSecretMasks({});
      setSecretSource("none");
      return;
    }
    if (!effectiveLinked && secretsPrimed) void refreshSecretKeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editMode, effectiveLinked, secretsPrimed]);

  useEffect(() => {
    if (!isOpen) return;
    setSecretsPrimed(false);
    setCapabilitiesPrimed(false);
  }, [isOpen, groupId, actorId]);

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
        setAttachProfileId(actorProfileIdentityKey({ id: profileId, scope: "global", owner_id: "" }));
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

  const handleUploadAvatar = async (file: File | null) => {
    if (!file || !groupId || !actorId) return;
    setAvatarBusy("upload");
    setSecretsError("");
    setLocalNotice("");
    try {
      const resp = await api.uploadActorAvatar(groupId, actorId, file);
      if (!resp.ok) {
        setSecretsError(resp.error?.message || t("avatarUploadFailed"));
        return;
      }
      await onAvatarChanged?.();
      setLocalNotice(t("avatarSaved"));
    } catch {
      setSecretsError(t("avatarUploadFailed"));
    } finally {
      setAvatarBusy("");
    }
  };

  const handleClearAvatar = async () => {
    if (!groupId || !actorId) return;
    setAvatarBusy("clear");
    setSecretsError("");
    setLocalNotice("");
    try {
      const resp = await api.clearActorAvatar(groupId, actorId);
      if (!resp.ok) {
        setSecretsError(resp.error?.message || t("saveFailed"));
        return;
      }
      await onAvatarChanged?.();
      setLocalNotice(t("avatarReset"));
    } catch {
      setSecretsError(t("saveFailed"));
    } finally {
      setAvatarBusy("");
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
          runner: normalizeActorRunner(selectedProfile?.runner || runner),
          setVars: {},
          unsetKeys: [],
          clear: false,
          capabilityAutoload: parseCapabilityIdInput(capabilityAutoloadText),
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
        runner,
        setVars: setParsed.setVars,
        unsetKeys: unsetParsed.unsetKeys,
        clear: secretsClearAll,
        capabilityAutoload: parseCapabilityIdInput(capabilityAutoloadText),
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

  const sectionCardClass = "rounded-2xl p-4 sm:p-5 glass-panel";
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const sectionHintClass = "mt-1 text-xs text-[var(--color-text-muted)]";
  const collapsibleSummaryClass = `flex cursor-pointer list-none items-start justify-between gap-3 [&::-webkit-details-marker]:hidden`;
  const collapsibleLabelClass = "text-xs font-medium text-[var(--color-text-secondary)]";
  const collapsibleChevronClass = "text-sm transition-transform group-open:rotate-180 text-[var(--color-text-tertiary)]";
  const nestedCardClass = "group rounded-xl border p-3 border-[var(--glass-border-subtle)] bg-[var(--glass-bg)] transition-all duration-300 hover:border-[var(--glass-border)]";
  const saveDisabled =
    busy === "actor-update" ||
    avatarBusy !== "" ||
    secretsBusy ||
    roleNotesBusy ||
    (editMode === "custom" && effectiveLinked) ||
    (editMode === "custom" && requireCommand && !command.trim()) ||
    (editMode === "profile" && !String(attachProfileId || "").trim());
  const showRuntimeSetup = !effectiveLinked && editMode === "custom" && runtime === "custom";
  const customRunnerLockedToPty = !supportsStandardWebHeadlessRuntime(runtime);
  const webModelRunnerLockedToHeadless = runtime === "web_model";

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onCancel}
      titleId="edit-actor-title"
      title={
        <div>
          <div className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("editAgent", { actorId })}
          </div>
          <div className="text-sm mt-1 text-[var(--color-text-muted)]">{t("changeSettings")}</div>
        </div>
      }
      closeAriaLabel={t("common:close")}
      panelClassName="w-full h-full sm:h-auto sm:w-[min(100vw-2rem,72rem)] sm:max-w-[72rem] sm:mt-6 sm:max-h-[calc(100vh-5rem)]"
      modalRef={modalRef}
      footerActions={
        <>
          {secretsError ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-rose-500/20 bg-rose-500/5 text-rose-600 dark:text-rose-400"
              role="alert"
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

          <div className="flex flex-col-reverse sm:flex-row gap-3 sm:justify-end">
            <Button
              type="button"
              variant="secondary"
              className="w-full sm:w-auto transition-all ease-spring duration-300"
              onClick={onCancel}
            >
              {t("common:cancel")}
            </Button>
            <Button
              type="button"
              variant="outline"
              className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
              onClick={() => void submit(true)}
              disabled={saveDisabled}
            >
              {t("saveAndRestart")}
            </Button>
            <Button
              type="button"
              className="w-full sm:w-auto font-semibold transition-all ease-spring duration-300"
              onClick={() => void submit(false)}
              disabled={saveDisabled}
            >
              {t("common:save")}
            </Button>
          </div>
        </>
      }
    >
      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-hide bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_30%),linear-gradient(180deg,rgb(251,250,247),rgb(245,244,241))] p-4 dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))] sm:p-6">
        <div className="mx-auto w-full max-w-6xl space-y-4">
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1.08fr)_minmax(22rem,0.92fr)] xl:items-start">
            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionBasics", "Basics")}</div>
              <div className={sectionHintClass}>
                {t("sectionBasicsHint", "Edit the actor label, built-in role preset seed, and stable role notes here.")}
              </div>

              <div className="mt-4 space-y-4">
                <div className="grid gap-4 sm:grid-cols-[88px_minmax(0,1fr)] sm:items-start">
                  <div className="justify-self-start">
                    <ActorAvatarField
                      label={null}
                      avatarUrl={avatarUrl}
                      runtime={runtime}
                      title={title || actorId}
                      isDark={isDark}
                      sizeClassName="h-16 w-16 sm:h-[4.5rem] sm:w-[4.5rem]"
                      disabled={busy === "actor-update" || avatarBusy !== ""}
                      resetDisabled={!hasCustomAvatar}
                      uploadBusy={avatarBusy === "upload"}
                      resetBusy={avatarBusy === "clear"}
                      onSelectFile={(file) => void handleUploadAvatar(file)}
                      onReset={() => void handleClearAvatar()}
                    />
                  </div>

                  <div className="min-w-0">
                    <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("displayName")}</label>
                    <Input
                      value={title}
                      onChange={(e) => onChangeTitle(e.target.value)}
                      placeholder={actorId}
                    />
                    <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("leaveEmptyForId")}</div>
                  </div>
                </div>

                <div>
                  <RolePresetPicker
                    draftValue={roleNotes}
                    onChangeDraft={onChangeRoleNotes}
                    disabled={roleNotesBusy || busy === "actor-update"}
                  />
                  <label className="block text-xs font-medium mt-3 mb-2 text-[var(--color-text-muted)]">{t("roleNotes")}</label>
                  <Textarea
                    className="min-h-[144px]"
                    value={roleNotes}
                    onChange={(e) => onChangeRoleNotes(e.target.value)}
                    placeholder={t("roleNotesPlaceholder")}
                    spellCheck={false}
                    disabled={roleNotesBusy}
                  />
                  <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("roleNotesHint")}</div>
                  {roleNotesBusy ? <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">{t("loadingRoleNotes")}</div> : null}
                </div>
              </div>
            </Surface>

            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionRuntime", "Runtime & Profile")}</div>
              <div className={sectionHintClass}>
                {t("sectionRuntimeHint", "Choose a runtime profile or custom runtime config. Role presets above only affect role notes.")}
              </div>

              <div className="mt-4">
                <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("creationMode")}</label>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  <Button type="button" variant="outline" className={modeButtonClass(editMode === "custom")} onClick={() => setEditMode("custom")}>
                    {t("customAgent")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    className={modeButtonClass(editMode === "profile")}
                    onClick={() => {
                      setEditMode("profile");
                      setPendingConvertToCustom(false);
                      setLocalNotice("");
                      if (!actorProfilesBusy && selectableActorProfiles.length <= 0) {
                        void onRequestActorProfiles?.();
                      }
                    }}
                  >
                    {t("fromActorProfile")}
                  </Button>
                </div>
              </div>

              <div className="mt-4 space-y-4">
                {editMode === "profile" ? (
                  <>
                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("actorProfile")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-3 py-2 text-sm min-h-[40px] glass-input text-[var(--color-text-primary)]"
                        value={attachProfileId}
                        onChange={setAttachProfileId}
                        disabled={actorProfilesBusy || busy === "actor-update"}
                        ariaLabel={t("actorProfile")}
                        items={[
                          { value: "", label: actorProfilesBusy ? t("loadingProfiles") : t("selectActorProfile") },
                          ...selectableActorProfiles.map((profile) => ({
                            value: actorProfileIdentityKey(profile),
                            label: `${profile.name || profile.id} · ${profileScopeLabel(profile, t)}`,
                          })),
                        ]}
                        searchable
                      />
                      {!actorProfilesBusy && actorProfiles.length > 0 && selectableActorProfiles.length === 0 ? (
                        <div className="mt-1.5 text-[10px] text-[var(--color-text-muted)]">
                          ChatGPT Web Model is managed directly in Settings &gt; ChatGPT Web Model.
                        </div>
                      ) : null}
                    </div>

                    {selectedProfile ? (
                      <Surface className="px-3 py-2 text-xs text-[var(--color-text-secondary)]" variant="subtle" radius="md" padding="none">
                        <div className="font-medium">{selectedProfile.name || selectedProfile.id}</div>
                        <div className="mt-1">{profileScopeLabel(selectedProfile, t)}</div>
                        <div className="mt-1 flex flex-wrap items-center gap-2">
                          <span>{RUNTIME_INFO[String(selectedProfile.runtime) as SupportedRuntime]?.label || selectedProfile.runtime}</span>
                          <span className="rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                            {selectedProfileRunner === "headless" ? t("headless") : t("pty", { defaultValue: "PTY" })}
                          </span>
                        </div>
                        {commandPreview(selectedProfile.command) ? <div className="mt-1 font-mono break-all">{commandPreview(selectedProfile.command)}</div> : null}
                      </Surface>
                    ) : null}
                  </>
                ) : effectiveLinked ? (
                  <Surface className="border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-3 text-[var(--color-text-primary)]" radius="md" padding="none">
                    <div className="text-sm font-medium">
                      {selectedProfileName ? t("managedByProfileName", { name: selectedProfileName }) : t("managedByProfile")}
                    </div>
                    <div className="mt-1 text-xs text-[var(--color-text-secondary)]">{t("managedByProfileCustomHint")}</div>
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
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("runtime")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                        value={runtime}
                        onChange={(value) => {
                          const next = value as SupportedRuntime;
                          onChangeRuntime(next);
                          if (next === "web_model") onChangeRunner("headless");
                          else if (!supportsStandardWebHeadlessRuntime(next)) onChangeRunner("pty");
                          const nextInfo = runtimes.find((r) => r.name === next);
                          const nextDefault = String(nextInfo?.recommended_command || "").trim();
                          onChangeCommand(nextDefault);
                        }}
                        ariaLabel={t("runtime")}
                        items={SUPPORTED_RUNTIMES.map((rt) => {
                          const info = RUNTIME_INFO[rt];
                          const rtInfoLocal = runtimes.find((r) => r.name === rt);
                          const runtimeAvailable = rtInfoLocal?.available ?? false;
                          const selectable = runtimeAvailable || rt === "custom";
                          return {
                            value: rt,
                            label: `${info?.label || rt}${!runtimeAvailable && rt !== "custom" ? ` ${t("notInstalled")}` : ""}`,
                            disabled: !selectable,
                          };
                        })}
                      />
                    </div>

	                    {supportsStandardWebHeadlessRuntime(runtime) ? (
	                      <div>
	                        <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                           {t("runnerMode", { defaultValue: "Runner mode" })}
                         </label>
                         <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                           <Button
                             type="button"
                             variant="outline"
                             className={modeButtonClass(runner === "pty")}
                             onClick={() => onChangeRunner("pty")}
                             disabled={webModelRunnerLockedToHeadless}
                           >
                             {t("pty", { defaultValue: "PTY" })}
                           </Button>
                           <Button
                             type="button"
                             variant="outline"
                             className={modeButtonClass(runner === "headless")}
                             onClick={() => onChangeRunner("headless")}
                             disabled={customRunnerLockedToPty}
                           >
                             {t("headless")}
                           </Button>
                         </div>
                         <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                           {webModelRunnerLockedToHeadless
                             ? t("runnerModeWebModelNote", { defaultValue: "ChatGPT Web Model runs through browser delivery and a remote MCP connector, so it is fixed to Headless." })
                             : customRunnerLockedToPty
                             ? t("runnerModeHeadlessNote", { defaultValue: "Only some runtimes, such as codex and claude, support Headless mode. Other runtimes are fixed to PTY." })
                             : t("runnerModeHint", { defaultValue: "PTY uses terminal interaction; Headless uses structured event flow." })}
	                        </div>
	                      </div>
	                    ) : null}

	                    {runtime === "web_model" ? (
	                      <div className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/20 bg-sky-500/5 text-sky-700 dark:text-sky-300">
	                        <div className="font-medium">
	                          {t("webModelActorBoundConnectorTitle", { defaultValue: "Single ChatGPT Web Model actor" })}
	                        </div>
	                        <div className="mt-1">
	                          {t("webModelActorBoundConnectorHint", {
	                            defaultValue:
	                              "Manage the ChatGPT MCP URL and target conversation in Settings > ChatGPT Web Model. CCCC supports one ChatGPT Web Model actor in this instance.",
	                          })}
	                        </div>
	                      </div>
	                    ) : null}

	                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("command")}</label>
                      <Input
                        className="font-mono"
                        value={command}
                        onChange={(e) => onChangeCommand(e.target.value)}
                        placeholder={defaultCommand || t("enterCommand")}
                      />
                      {isRunning ? <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("runtimeChangesNote")}</div> : null}
                      {defaultCommand.trim() ? (
                        <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                          {t("default")} <code className="px-1 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">{defaultCommand}</code>
                        </div>
                      ) : null}
                    </div>
                  </div>
                )}
              </div>
            </Surface>
            </div>

            <details className={`group ${sectionCardClass}`}>
              <summary className={collapsibleSummaryClass}>
                <div>
                  <div className={sectionTitleClass}>{t("sectionAdvanced", "Advanced")}</div>
                  <div className={sectionHintClass}>{t("sectionAdvancedHint", "Keep low-frequency configuration here so the main edit path stays short.")}</div>
                </div>
                <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
              </summary>

              <div className="mt-4 space-y-4 border-t border-[var(--glass-border-subtle)] pt-4">
                {showRuntimeSetup ? (
                  <details className={nestedCardClass}>
                    <summary className={collapsibleSummaryClass}>
                      <div>
                        <div className={collapsibleLabelClass}>{t("runtimeSetupSection", "Connection setup")}</div>
                        <div className={sectionHintClass}>{t("runtimeSetupSectionHint", "Show runtime-specific MCP setup only when you need to wire a client manually.")}</div>
                      </div>
                      <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                    </summary>
                    <div className="mt-4 rounded-xl border px-3 py-2 text-[11px] border-amber-500/20 bg-amber-500/5 text-amber-700 dark:text-amber-400">
                      <div className="font-medium">{t("manualMcpRequired")}</div>
                      {runtime === "custom" ? (
                        <div className="mt-1">
                          {t("configureMcpStdio")} <code className="px-1 rounded bg-amber-500/15">cccc</code> {t("thatRuns")} <code className="px-1 rounded bg-amber-500/15">cccc mcp</code>.
                        </div>
                      ) : null}
                      {runtime === "custom" ? (
                        <pre className="mt-1.5 p-2 rounded overflow-x-auto whitespace-pre bg-amber-500/10 text-amber-800 dark:text-amber-100">
                          <code>{BASIC_MCP_CONFIG_SNIPPET}</code>
                        </pre>
                      ) : null}
                      <div className="mt-1 text-[10px] text-amber-700/80 dark:text-amber-400/80">{t("restartAfterConfig")}</div>
                    </div>
                  </details>
                ) : null}

                {editMode === "custom" ? (
                  <details
                    className={nestedCardClass}
                    onToggle={(event) => {
                      const open = (event.currentTarget as HTMLDetailsElement).open;
                      if (!open || secretsPrimed) return;
                      setSecretsPrimed(true);
                      void refreshSecretKeys();
                    }}
                  >
                    <summary className={collapsibleSummaryClass}>
                      <div>
                        <div className={collapsibleLabelClass}>{t("secretsSection", "Secrets & Environment")}</div>
                        <div className={sectionHintClass}>{t("secretsSectionHint", "Only open this when you need to rotate keys or change private runtime environment.")}</div>
                      </div>
                      <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                    </summary>
                    <div className="mt-4">
                      <div className="flex items-center justify-between gap-3">
                        <label className="block text-xs font-medium text-[var(--color-text-muted)]">{t("secretsWriteOnly")}</label>
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => void refreshSecretKeys()}
                          disabled={secretsBusy}
                          title={t("refreshConfiguredKeys")}
                        >
                          {t("refresh")}
                        </Button>
                      </div>
                      <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                        {t("secretsStoredLocallyEdit").replace(/<1>|<\/1>/g, "")} {secretKeys.length ? (
                          <>
                            {t("configuredKeys")}
                            <div className="mt-1.5 flex flex-wrap gap-1.5">
                              {secretKeys.map((key) => {
                                const masked = String(secretMasks[key] || "******");
                                return (
                                  <span
                                    key={key}
                                    className="inline-flex items-center rounded-md border px-2 py-0.5 font-mono text-[10px] border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)] bg-[var(--glass-panel-bg)]"
                                    title={`${t("maskedPreview")}: ${masked}`}
                                  >
                                    {key}
                                  </span>
                                );
                              })}
                            </div>
                            <div className="mt-1">{t("hoverToPreviewMasked")}</div>
                            {secretSource === "profile-preview" ? <div className="mt-1">{t("pendingConvertSecretsPreview")}</div> : null}
                          </>
                        ) : (
                          <>{t("noKeysConfigured")}</>
                        )}
                      </div>
                      <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">{t("secretsAppliedNote").replace(/<1>|<\/1>/g, "")}</div>

                      <label className="block text-[11px] font-medium mt-3 mb-1.5 text-[var(--color-text-secondary)]">{t("setUpdate")}</label>
                      <Textarea
                        className="min-h-[96px] font-mono"
                        value={secretsSetText}
                        onChange={(e) => setSecretsSetText(e.target.value)}
                        placeholder={secretsPlaceholder.set}
                      />

                      <label className="block text-[11px] font-medium mt-3 mb-1.5 text-[var(--color-text-secondary)]">{t("unset")}</label>
                      <Textarea
                        className="min-h-[72px] font-mono"
                        value={secretsUnsetText}
                        onChange={(e) => setSecretsUnsetText(e.target.value)}
                        placeholder={secretsPlaceholder.unset}
                      />

                      <label className="flex items-center gap-2 text-[11px] font-medium mt-3 text-[var(--color-text-secondary)]">
                        <input
                          type="checkbox"
                          checked={secretsClearAll}
                          onChange={(e) => setSecretsClearAll(e.target.checked)}
                          disabled={secretsBusy || busy === "actor-update"}
                        />
                        {t("clearAllKeys")}
                      </label>
                    </div>
                  </details>
                ) : null}

                <details
                  className={nestedCardClass}
                  onToggle={(event) => {
                    const open = (event.currentTarget as HTMLDetailsElement).open;
                    if (!open || capabilitiesPrimed) return;
                    setCapabilitiesPrimed(true);
                  }}
                >
                  <summary className={collapsibleSummaryClass}>
                    <div>
                      <div className={collapsibleLabelClass}>{t("capabilitiesSection", "Capabilities")}</div>
                      <div className={sectionHintClass}>{t("capabilitiesSectionHint", "Only open this when you need to change autoloaded capabilities for the actor.")}</div>
                    </div>
                    <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                  </summary>
                  <div className="mt-4">
                    <CapabilityPicker
                      isDark={isDark}
                      value={parseCapabilityIdInput(capabilityAutoloadText)}
                      onChange={(next) => onChangeCapabilityAutoloadText(formatCapabilityIdInput(next))}
                      disabled={busy === "actor-update"}
                      active={capabilitiesPrimed}
                      label={t("autoloadCapabilities")}
                      hint={t("autoloadCapabilitiesHint")}
                    />
                  </div>
                </details>

                {editMode === "custom" && runtime === "web_model" ? (
                  <div className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/20 bg-sky-500/5 text-sky-700 dark:text-sky-300">
                    ChatGPT Web Model is managed in Settings &gt; ChatGPT Web Model instead of being saved as a Runtime Profile.
                  </div>
                ) : editMode === "custom" ? (
                  <details className={nestedCardClass}>
                    <summary className={collapsibleSummaryClass}>
                      <div>
                        <div className={collapsibleLabelClass}>{t("profileToolsSection", "Profile tools")}</div>
                        <div className={sectionHintClass}>{t("profileToolsSectionHint", "Save the current custom runtime setup into the reusable actor profile library.")}</div>
                      </div>
                      <span aria-hidden="true" className={collapsibleChevronClass}>⌄</span>
                    </summary>
                    <div className="mt-4 flex flex-wrap gap-3">
                      <Button
                        type="button"
                        variant="secondary"
                        onClick={() => void saveAsProfile()}
                        disabled={busy === "actor-profile-save" || busy === "actor-update"}
                      >
                        {busy === "actor-profile-save" ? t("savingProfile") : t("addToActorProfiles")}
                      </Button>
                    </div>
                  </details>
                ) : null}
              </div>
            </details>
          </div>
        </div>
      </ModalFrame>
  );
}
