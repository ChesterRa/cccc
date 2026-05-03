import { useEffect, useMemo, useState } from "react";
import {
  ActorProfile,
  RuntimeInfo,
  SupportedRuntime,
  SUPPORTED_RUNTIMES,
  RUNTIME_INFO,
} from "../../types";
import { useTranslation } from "react-i18next";
import { BASIC_MCP_CONFIG_SNIPPET } from "../../utils/mcpConfigSnippets";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { CapabilityPicker } from "../CapabilityPicker";
import { RolePresetPicker } from "../RolePresetPicker";
import { ActorAvatarField } from "../ActorAvatarField";
import { SelectCombobox } from "../SelectCombobox";
import { formatCapabilityIdInput, parseCapabilityIdInput } from "../../utils/capabilityAutoload";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { supportsStandardWebHeadlessRuntime } from "../../utils/headlessRuntimeSupport";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Surface } from "../ui/surface";
import { Textarea } from "../ui/textarea";

export interface AddActorModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  hasForeman: boolean;
  runtimes: RuntimeInfo[];

  suggestedActorId: string;
  newActorId: string;
  setNewActorId: (id: string) => void;

  newActorRole: "peer" | "foreman";
  setNewActorRole: (role: "peer" | "foreman") => void;

  newActorUseProfile: boolean;
  setNewActorUseProfile: (v: boolean) => void;
  newActorProfileId: string;
  setNewActorProfileId: (id: string) => void;
  actorProfiles: ActorProfile[];
  actorProfilesBusy: boolean;

  newActorRuntime: SupportedRuntime;
  setNewActorRuntime: (runtime: SupportedRuntime) => void;
  newActorRunner: "pty" | "headless";
  setNewActorRunner: (runner: "pty" | "headless") => void;

  newActorCommand: string;
  setNewActorCommand: (cmd: string) => void;
  newActorUseDefaultCommand: boolean;
  setNewActorUseDefaultCommand: (v: boolean) => void;

  newActorSecretsSetText: string;
  setNewActorSecretsSetText: (v: string) => void;
  newActorCapabilityAutoloadText: string;
  setNewActorCapabilityAutoloadText: (v: string) => void;
  newActorRoleNotes: string;
  setNewActorRoleNotes: (v: string) => void;

  showAdvancedActor: boolean;
  setShowAdvancedActor: (show: boolean) => void;

  addActorError: string;
  setAddActorError: (msg: string) => void;

  canAddActor: boolean;
  addActorDisabledReason: string;

  onAddActor: (avatarFile?: File | null) => Promise<boolean> | boolean;
  onSaveAsProfile: () => void;
  onClose: () => void;
  onCancelAndReset: () => void;
}

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
    "px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-colors",
    selected
      ? "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-[rgb(35,36,37)] dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white"
      : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
  ].join(" ");
}

function secretsPlaceholderForRuntime(runtime: SupportedRuntime): string {
  if (runtime === "claude") {
    return 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."';
  }
  if (runtime === "codex") {
    return "# Configure OpenAI-compatible Codex providers with Codex config or command -c overrides.";
  }
  if (runtime === "gemini") {
    return 'GOOGLE_API_KEY="..."';
  }
  return 'ANTHROPIC_AUTH_TOKEN="..."\nANTHROPIC_BASE_URL="..."';
}

export function AddActorModal({
  isOpen,
  isDark,
  busy,
  hasForeman,
  runtimes,
  suggestedActorId,
  newActorId,
  setNewActorId,
  newActorRole,
  setNewActorRole,
  newActorUseProfile,
  setNewActorUseProfile,
  newActorProfileId,
  setNewActorProfileId,
  actorProfiles,
  actorProfilesBusy,
  newActorRuntime,
  setNewActorRuntime,
  newActorRunner,
  setNewActorRunner,
  newActorCommand,
  setNewActorCommand,
  newActorUseDefaultCommand,
  setNewActorUseDefaultCommand,
  newActorSecretsSetText,
  setNewActorSecretsSetText,
  newActorCapabilityAutoloadText,
  setNewActorCapabilityAutoloadText,
  newActorRoleNotes,
  setNewActorRoleNotes,
  showAdvancedActor,
  setShowAdvancedActor,
  addActorError,
  setAddActorError,
  canAddActor,
  addActorDisabledReason,
  onAddActor,
  onSaveAsProfile,
  onClose,
  onCancelAndReset,
}: AddActorModalProps) {
  const { t } = useTranslation("actors");
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
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
    if (!isOpen || !newActorUseProfile || !String(newActorProfileId || "").trim()) return;
    const selected = selectableActorProfiles.some((profile) => actorProfileIdentityKey(profile) === String(newActorProfileId || "").trim());
    if (!selected) setNewActorProfileId("");
  }, [isOpen, newActorProfileId, newActorUseProfile, selectableActorProfiles, setNewActorProfileId]);

  if (!isOpen) return null;

  const runtimeInfo = runtimes.find((r) => r.name === newActorRuntime);
  const runtimeAvailable = runtimeInfo?.available ?? false;
  const defaultCommand = runtimeInfo?.recommended_command || "";
  const newActorSecretsPlaceholder = secretsPlaceholderForRuntime(newActorRuntime);
  const selectedProfile = selectableActorProfiles.find((item) => actorProfileIdentityKey(item) === String(newActorProfileId || "").trim());
  const selectedProfileRuntime = String(selectedProfile?.runtime || "").trim() as SupportedRuntime;
  const selectedProfileCommand = commandPreview(selectedProfile?.command);
  const showRuntimeSetup = !newActorUseProfile && newActorRuntime === "custom";
  const showCommandEditor = !newActorUseProfile && (newActorRuntime === "custom" || !newActorUseDefaultCommand);
  const previewRuntime = newActorUseProfile ? selectedProfileRuntime || null : newActorRuntime;
  const previewTitle = String(newActorId || "").trim() || suggestedActorId;
  const customRunnerLockedToPty = !newActorUseProfile && !supportsStandardWebHeadlessRuntime(newActorRuntime);
  const webModelRunnerLockedToHeadless = !newActorUseProfile && newActorRuntime === "web_model";
  const webModelSetupIsActorBound = !newActorUseProfile && newActorRuntime === "web_model";

  const sectionCardClass = "rounded-2xl p-4 sm:p-5 glass-panel";
  const sectionTitleClass = "text-sm font-semibold text-[var(--color-text-primary)]";
  const sectionHintClass = "mt-1 text-xs text-[var(--color-text-muted)]";
  const collapsibleSummaryClass =
    "flex cursor-pointer list-none items-start justify-between gap-3 [&::-webkit-details-marker]:hidden";
  const collapsibleLabelClass = "text-xs font-medium text-[var(--color-text-secondary)]";
  const collapsibleChevronClass =
    "text-sm transition-transform group-open:rotate-180 text-[var(--color-text-tertiary)]";
  const nestedCardClass = "rounded-xl border p-3 border-[var(--glass-border-subtle)] bg-[var(--glass-bg)]";

  const handleSubmit = async () => {
    try {
      const ok = await Promise.resolve(onAddActor(avatarFile));
      if (ok) setAvatarFile(null);
    } catch (e) {
      setAddActorError(e instanceof Error ? e.message : t("failedToAddAgent"));
    }
  };

  const handleCancel = () => {
    setAvatarFile(null);
    onCancelAndReset();
  };

  return (
    <div
      className="fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in glass-overlay"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-actor-title"
    >
      <div
        ref={modalRef}
        className="w-full h-full sm:h-auto sm:max-w-2xl sm:mt-10 sm:max-h-[calc(100vh-5rem)] border border-[var(--glass-border-subtle)] shadow-2xl animate-scale-in rounded-none sm:rounded-2xl glass-modal flex flex-col overflow-hidden text-[var(--color-text-primary)]"
      >
        <div className="px-6 py-4 border-b safe-area-inset-top border-[var(--glass-border-subtle)] glass-header flex-shrink-0">
          <div id="add-actor-title" className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t("addAiAgent")}
          </div>
          <div className="text-sm mt-1 text-[var(--color-text-muted)]">{t("addActorSubtitle")}</div>
        </div>

        <div className="flex-1 min-h-0 overflow-y-auto bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.92),rgba(255,255,255,0)_30%),linear-gradient(180deg,rgb(251,250,247),rgb(245,244,241))] p-4 dark:bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.05),rgba(255,255,255,0)_34%),linear-gradient(180deg,rgba(17,18,22,0.98),rgba(11,12,15,1))] sm:p-6 safe-area-bottom-compact">
          <div className="mx-auto max-w-2xl space-y-4">
            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionBasics")}</div>
              <div className={sectionHintClass}>{t("addSectionBasicsHint")}</div>

              <div className="mt-4 space-y-4">
                <div className="flex flex-col gap-5 sm:flex-row sm:items-start">
                  <div className="sm:w-[104px] sm:flex-shrink-0">
                    <ActorAvatarField
                      label={null}
                      avatarUrl={undefined}
                      previewUrl={avatarPreviewUrl}
                      runtime={previewRuntime}
                      title={previewTitle}
                      isDark={isDark}
                      sizeClassName="h-14 w-14"
                      disabled={busy === "actor-add"}
                      resetDisabled={!avatarFile}
                      onSelectFile={setAvatarFile}
                      onReset={() => setAvatarFile(null)}
                    />
                  </div>

                  <div className="min-w-0 flex-1">
                    <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                      {t("agentName")} <span className="text-[var(--color-text-muted)]">{t("unicodeSupport")}</span>
                    </label>
                    <Input
                      value={newActorId}
                      onChange={(e) => setNewActorId(e.target.value)}
                      placeholder={suggestedActorId}
                    />
                    <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                      {t("leaveEmptyToUse")}{" "}
                      <code className="px-1 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
                        {suggestedActorId}
                      </code>
                    </div>
                  </div>
                </div>

                <div>
                  <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("role")}</label>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                    <Button
                      type="button"
                      className={classNames(
                        "h-auto justify-start whitespace-normal border text-sm font-medium transition-all",
                        newActorRole === "foreman"
                          ? "bg-amber-500/20 border-amber-500 text-amber-700 dark:text-amber-300"
                          : hasForeman
                            ? "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-muted)] cursor-not-allowed"
                            : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                      )}
                      onClick={() => {
                        if (!hasForeman) setNewActorRole("foreman");
                      }}
                      disabled={hasForeman}
                    >
                      {t("foremanRole")} {hasForeman && t("foremanExists")}
                    </Button>
                    <Button
                      type="button"
                      className={classNames(
                        "h-auto justify-start whitespace-normal border text-sm font-medium transition-all",
                        newActorRole === "peer"
                          ? "border-[rgb(35,36,37)] bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white"
                          : !hasForeman
                            ? "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-muted)] cursor-not-allowed"
                            : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                      )}
                      onClick={() => {
                        if (hasForeman) setNewActorRole("peer");
                      }}
                      disabled={!hasForeman}
                    >
                      {t("peerRole")} {!hasForeman && t("needForemanFirst")}
                    </Button>
                  </div>
                  <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                    {hasForeman ? t("foremanLeads") : t("firstAgentForeman")}
                  </div>
                </div>

                <RolePresetPicker
                  draftValue={newActorRoleNotes}
                  onChangeDraft={setNewActorRoleNotes}
                  disabled={busy === "actor-add"}
                />

                <div>
                  <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("roleNotes")}</label>
                  <Textarea
                    className="min-h-[144px]"
                    value={newActorRoleNotes}
                    onChange={(e) => setNewActorRoleNotes(e.target.value)}
                    placeholder={t("roleNotesPlaceholder")}
                    spellCheck={false}
                  />
                  <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">{t("newActorRoleNotesHint")}</div>
                </div>
              </div>
            </Surface>

            <Surface className={sectionCardClass}>
              <div className={sectionTitleClass}>{t("sectionRuntime")}</div>
              <div className={sectionHintClass}>{t("sectionRuntimeHint")}</div>

              <div className="mt-4">
                <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("creationMode")}</label>
                <div className="grid grid-cols-2 gap-2">
                  <Button type="button" variant="outline" className={modeButtonClass(!newActorUseProfile)} onClick={() => setNewActorUseProfile(false)}>
                    {t("customAgent")}
                  </Button>
                  <Button type="button" variant="outline" className={modeButtonClass(newActorUseProfile)} onClick={() => setNewActorUseProfile(true)}>
                    {t("fromActorProfile")}
                  </Button>
                </div>
              </div>

              <div className="mt-4 space-y-4">
                {newActorUseProfile ? (
                  <>
                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("actorProfile")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                        value={newActorProfileId}
                        onChange={setNewActorProfileId}
                        disabled={actorProfilesBusy}
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
                      <Surface className="px-3 py-3 text-[var(--color-text-secondary)]" variant="subtle" radius="md" padding="none">
                        <div className="text-sm font-medium text-[var(--color-text-primary)]">
                          {selectedProfile.name || selectedProfile.id}
                        </div>
                        <div className="mt-1 text-xs">
                          {profileScopeLabel(selectedProfile, t)}
                        </div>
                        <div className="mt-1 text-xs">
                          {RUNTIME_INFO[selectedProfileRuntime]?.label || selectedProfile.runtime}
                        </div>
                        {selectedProfileCommand ? (
                          <div className="mt-2 font-mono text-[11px] break-all text-[var(--color-text-tertiary)]">
                            {selectedProfileCommand}
                          </div>
                        ) : null}
                      </Surface>
                    ) : null}
                  </>
                ) : (
                  <>
                    <div>
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">{t("aiRuntime")}</label>
                      <SelectCombobox
                        className="w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors glass-input text-[var(--color-text-primary)]"
                        value={newActorRuntime}
                        onChange={(value) => {
                          const next = value as SupportedRuntime;
                          setNewActorRuntime(next);
                          if (next === "web_model") setNewActorRunner("headless");
                          else if (!supportsStandardWebHeadlessRuntime(next)) setNewActorRunner("pty");
                          setNewActorCommand("");
                          setNewActorUseDefaultCommand(next !== "custom");
                        }}
                        ariaLabel={t("aiRuntime")}
                        items={SUPPORTED_RUNTIMES.map((rt) => {
                          const info = RUNTIME_INFO[rt];
                          const rtInfo = runtimes.find((r) => r.name === rt);
                          const available = rtInfo?.available ?? false;
                          const selectable = available || rt === "custom";
                          return {
                            value: rt,
                            label: `${info?.label || rt}${!available && rt !== "custom" ? ` ${t("notInstalled")}` : ""}`,
                            disabled: !selectable,
                          };
                        })}
                      />
                      {RUNTIME_INFO[newActorRuntime]?.desc ? (
                        <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                          {RUNTIME_INFO[newActorRuntime].desc}
                        </div>
                      ) : null}
                    </div>

                    {newActorRuntime !== "custom" ? (
                      <label className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
                        <input
                          type="checkbox"
                          checked={newActorUseDefaultCommand}
                          onChange={(e) => {
                            const checked = e.target.checked;
                            setNewActorUseDefaultCommand(checked);
                            if (checked) setNewActorCommand("");
                          }}
                        />
                        {t("useRuntimeDefaultCommand")}
                      </label>
                    ) : null}

	                    {supportsStandardWebHeadlessRuntime(newActorRuntime) ? (
	                      <div>
	                        <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                          {t("runnerMode", { defaultValue: "Runner mode" })}
                        </label>
                        <div className="grid grid-cols-2 gap-2">
                          <Button
                            type="button"
                            variant="outline"
                            className={modeButtonClass(newActorRunner === "pty")}
                            onClick={() => setNewActorRunner("pty")}
                            disabled={webModelRunnerLockedToHeadless}
                          >
                            {t("pty", { defaultValue: "PTY" })}
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            className={modeButtonClass(newActorRunner === "headless")}
                            onClick={() => setNewActorRunner("headless")}
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

	                    {newActorRuntime === "web_model" ? (
	                      <div className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/25 bg-sky-500/10 text-sky-800 dark:text-sky-200">
	                        <div className="font-medium">
	                          {t("webModelActorBoundConnectorTitle", { defaultValue: "Single ChatGPT Web Model actor" })}
	                        </div>
	                        <div className="mt-1">
	                          {t("webModelActorBoundConnectorHint", {
	                            defaultValue:
	                              "CCCC supports one ChatGPT Web Model actor in this instance. Create it first, then open Settings > ChatGPT Web Model to copy the MCP URL and bind a ChatGPT conversation.",
	                          })}
	                        </div>
	                      </div>
	                    ) : null}

	                    {showCommandEditor ? (
                      <div>
                        <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                          {t("commandOverrideOptional")}
                        </label>
                        <Input
                          className="font-mono"
                          value={newActorCommand}
                          onChange={(e) => setNewActorCommand(e.target.value)}
                          placeholder={defaultCommand || t("enterCommand")}
                        />
                      </div>
                    ) : null}

                    {defaultCommand.trim() ? (
                      <div className="text-[10px] text-[var(--color-text-muted)]">
                        {newActorUseDefaultCommand && newActorRuntime !== "custom"
                          ? t("usingRuntimeDefaultCommand")
                          : t("default")}{" "}
                        <code className="px-1 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
                          {defaultCommand}
                        </code>
                      </div>
                    ) : null}

                    {newActorRuntime === "custom" || !runtimeAvailable ? (
                      <div className="rounded-xl border px-3 py-2 text-[11px] border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300">
                        <div className="font-medium">{t("manualMcpRequired")}</div>
                        <div className="mt-1">{t("customCommandHint").replace(/<1>|<\/1>/g, "")}</div>
                      </div>
                    ) : null}
                  </>
                )}
              </div>
            </Surface>

            {!newActorUseProfile ? (
              <details
                className={`group ${sectionCardClass}`}
                open={showAdvancedActor}
                onToggle={(e) => setShowAdvancedActor((e.currentTarget as HTMLDetailsElement).open)}
              >
                <summary className={collapsibleSummaryClass}>
                  <div>
                    <div className={sectionTitleClass}>{t("sectionAdvanced")}</div>
                    <div className={sectionHintClass}>{t("sectionAdvancedHint")}</div>
                  </div>
                  <span aria-hidden="true" className={collapsibleChevronClass}>
                    ⌄
                  </span>
                </summary>

                <div className="mt-4 space-y-4 border-t border-[var(--glass-border-subtle)] pt-4">
                  {showRuntimeSetup ? (
                    <details className={`group ${nestedCardClass}`}>
                      <summary className={collapsibleSummaryClass}>
                        <div>
                          <div className={collapsibleLabelClass}>{t("runtimeSetupSection")}</div>
                          <div className={sectionHintClass}>{t("runtimeSetupSectionHint")}</div>
                        </div>
                        <span aria-hidden="true" className={collapsibleChevronClass}>
                          ⌄
                        </span>
                      </summary>

                      <div className="mt-4 rounded-xl border px-3 py-2 text-[11px] border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300">
                        <div className="font-medium">{t("manualMcpRequired")}</div>
                        {newActorRuntime === "custom" ? (
                          <>
                            <div className="mt-1">{t("customCommandHint").replace(/<1>|<\/1>/g, "")}</div>
                            <div className="mt-1">
                              {t("configureMcpStdio")}{" "}
                              <code className="px-1 rounded bg-amber-500/15">cccc</code> {t("thatRuns")}{" "}
                              <code className="px-1 rounded bg-amber-500/15">cccc mcp</code>.
                            </div>
                          </>
                        ) : null}

                        {newActorRuntime === "custom" ? (
                          <pre className="mt-1.5 p-2 rounded overflow-x-auto whitespace-pre bg-amber-500/10 text-amber-800 dark:text-amber-200">
                            <code>{BASIC_MCP_CONFIG_SNIPPET}</code>
                          </pre>
                        ) : null}

                        <div className="mt-1 text-[10px] text-amber-700/80 dark:text-amber-300/80">
                          {t("restartAfterConfig")}
                        </div>
                      </div>
                    </details>
                  ) : null}

                  <details className={`group ${nestedCardClass}`}>
                    <summary className={collapsibleSummaryClass}>
                      <div>
                        <div className={collapsibleLabelClass}>{t("capabilitiesSection")}</div>
                        <div className={sectionHintClass}>{t("capabilitiesSectionHint")}</div>
                      </div>
                      <span aria-hidden="true" className={collapsibleChevronClass}>
                        ⌄
                      </span>
                    </summary>

                    <div className="mt-4">
                      <CapabilityPicker
                        isDark={isDark}
                        value={parseCapabilityIdInput(newActorCapabilityAutoloadText)}
                        onChange={(next) => setNewActorCapabilityAutoloadText(formatCapabilityIdInput(next))}
                        disabled={busy === "actor-add"}
                        label={t("autoloadCapabilities")}
                        hint={t("autoloadCapabilitiesHint")}
                      />
                    </div>
                  </details>

                  <details className={`group ${nestedCardClass}`}>
                    <summary className={collapsibleSummaryClass}>
                      <div>
                        <div className={collapsibleLabelClass}>{t("secretsSection")}</div>
                        <div className={sectionHintClass}>{t("secretsSectionHint")}</div>
                      </div>
                      <span aria-hidden="true" className={collapsibleChevronClass}>
                        ⌄
                      </span>
                    </summary>

                    <div className="mt-4">
                      <label className="block text-xs font-medium mb-2 text-[var(--color-text-muted)]">
                        {t("secretsWriteOnly")}
                      </label>
                      <Textarea
                        className="min-h-[112px] font-mono"
                        value={newActorSecretsSetText}
                        onChange={(e) => setNewActorSecretsSetText(e.target.value)}
                        placeholder={newActorSecretsPlaceholder}
                        spellCheck={false}
                      />
                      <div className="text-[10px] mt-1.5 text-[var(--color-text-muted)]">
                        {t("secretsStoredLocally").replace(/<1>|<\/1>/g, "")}
                      </div>
                      <div className="text-[10px] mt-1 text-[var(--color-text-muted)]">
                        {t("secretsFormat").replace(/<1>|<\/1>|<2>|<\/2>/g, "")}
                      </div>
                    </div>
                  </details>

                  {webModelSetupIsActorBound ? (
                    <div className="rounded-xl border px-3 py-2 text-[11px] border-sky-500/25 bg-sky-500/10 text-sky-800 dark:text-sky-200">
                      ChatGPT Web Model is a single actor for this CCCC instance. Create it, then manage the MCP URL and target ChatGPT chat in Settings &gt; ChatGPT Web Model.
                    </div>
                  ) : (
                    <details className={`group ${nestedCardClass}`}>
                      <summary className={collapsibleSummaryClass}>
                        <div>
                          <div className={collapsibleLabelClass}>{t("profileToolsSection")}</div>
                          <div className={sectionHintClass}>{t("profileToolsSectionHint")}</div>
                        </div>
                        <span aria-hidden="true" className={collapsibleChevronClass}>
                          ⌄
                        </span>
                      </summary>

                      <div className="mt-4 flex flex-wrap gap-3">
                        <Button
                          type="button"
                          variant="secondary"
                          onClick={onSaveAsProfile}
                          disabled={busy === "actor-profile-save" || busy === "actor-add"}
                        >
                          {busy === "actor-profile-save" ? t("savingProfile") : t("addToActorProfiles")}
                        </Button>
                      </div>
                    </details>
                  )}
                </div>
              </details>
            ) : null}
          </div>
        </div>

        <div className="border-t px-4 py-3 sm:px-6 sm:py-4 safe-area-inset-bottom border-[var(--glass-border-subtle)] glass-header flex-shrink-0">
          {addActorError ? (
            <div
              className="mb-3 rounded-xl border px-3 py-2 text-xs border-rose-500/30 bg-rose-500/10 text-rose-700 dark:text-rose-300"
              role="alert"
            >
              <div className="flex items-start justify-between gap-3">
                <span>{addActorError}</span>
                <button
                  type="button"
                  className="text-rose-700 dark:text-rose-300 hover:opacity-80"
                  onClick={() => setAddActorError("")}
                  aria-label={t("common:close")}
                >
                  ×
                </button>
              </div>
            </div>
          ) : null}

          <div className="flex flex-col-reverse sm:flex-row gap-3">
              <Button
                type="button"
                variant="secondary"
                onClick={handleCancel}
              >
                {t("common:cancel")}
              </Button>

            <div className="flex-1 min-w-0">
              <Button
                type="button"
                className="w-full font-semibold"
                onClick={() => {
                  void handleSubmit();
                }}
                disabled={!canAddActor}
              >
                {busy === "actor-add" ? t("adding") : newActorUseProfile ? t("createFromProfile") : t("addAgent")}
              </Button>
              {addActorDisabledReason ? (
                <div className="text-[10px] text-amber-600 dark:text-amber-300 mt-1.5">{addActorDisabledReason}</div>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
