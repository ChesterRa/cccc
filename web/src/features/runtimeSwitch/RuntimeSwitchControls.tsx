import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { AcpRuntimeMode } from "../../components/modals/AcpRuntimeMode";
import { ActorWorkingDirectory } from "../../components/modals/ActorWorkingDirectory";
import { RuntimeSelector } from "../../components/modals/RuntimeSelector";
import {
  RuntimeProfilePicker,
  RuntimeConfigurationModePicker,
} from "../../components/modals/RuntimeProfileControls";
import { SelectCombobox } from "../../components/SelectCombobox";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { useModalA11y } from "../../hooks/useModalA11y";
import * as api from "../../services/api";
import { fetchLocalSessions } from "../../services/api/localSessions";
import { useGroupStore } from "../../stores";
import {
  isWebModelRuntime,
  supportsAcpMode,
  SUPPORTED_RUNTIMES,
  type Actor,
  type ActorProfile,
  type GroupDoc,
  type LocalSessionSource,
  type RuntimeInfo,
  type RuntimeMode,
  type SupportedRuntime,
} from "../../types";
import { actorGenerationIdentity } from "../../utils/actorIdentity";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { runtimeDetectionKey } from "../../utils/runtimeDetection";
import {
  joinRuntimeSentences,
  runtimeSwitchErrorMessage,
  runtimeSwitchStartError,
} from "./runtimeSwitchOutcome";
import { refreshRuntimeSwitchGroup } from "./runtimeSwitchState";

const MANAGED_RUNTIMES = SUPPORTED_RUNTIMES.filter((runtime) => !isWebModelRuntime(runtime));

export function RuntimeSwitchControls({
  groupId,
  actor,
  targetKind,
  isDark,
  dirty,
  disabled,
  runtimes,
  profiles,
  profilesBusy,
  onRequestProfiles,
  onSaveCurrentProfile,
  onOutcomeUnknown,
  onSwitched,
  onCurrentSourceDirectory,
  scopes = [],
  activeScopeKey,
}: {
  groupId: string;
  actor: Actor;
  targetKind: "native" | "managed";
  isDark: boolean;
  dirty: boolean;
  disabled: boolean;
  runtimes: RuntimeInfo[];
  profiles: ActorProfile[];
  profilesBusy: boolean;
  onRequestProfiles?: () => Promise<void> | void;
  onSaveCurrentProfile?: () => Promise<boolean>;
  onOutcomeUnknown?: () => void;
  onSwitched?: () => void;
  onCurrentSourceDirectory?: (directory: string) => void;
  scopes?: NonNullable<GroupDoc["scopes"]>;
  activeScopeKey?: string;
}) {
  const { t } = useTranslation("actors");
  const [sources, setSources] = useState<LocalSessionSource[]>([]);
  const [members, setMembers] = useState<Actor[]>([]);
  const [sourceId, setSourceId] = useState(actor.external_session_id || "");
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [runtime, setRuntime] = useState<SupportedRuntime>("codex");
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode>("default");
  const [command, setCommand] = useState("");
  const [defaultScopeKey, setDefaultScopeKey] = useState(actor.default_scope_key || "");
  const [directoryBusy, setDirectoryBusy] = useState(false);
  const [configMode, setConfigMode] = useState<"custom" | "profile">("custom");
  const [profileKey, setProfileKey] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [noStart, setNoStart] = useState(false);
  const [busy, setBusy] = useState(false);
  const [profileBusy, setProfileBusy] = useState(false);
  const [profileSaveFailed, setProfileSaveFailed] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [blocked, setBlocked] = useState(false);
  const [result, setResult] = useState<api.ActorRuntimeSwitchResult | null>(null);
  const runtimeDetectionStatus = useGroupStore((state) => state.runtimeDetectionStatus);

  useEffect(() => {
    if (targetKind !== "native") return;
    let active = true;
    setLoading(true);
    setLoadError(false);
    void Promise.all([fetchLocalSessions(), api.fetchActors(groupId, false, { noCache: true })])
      .then(([sessions, actors]) => {
        if (!active) return;
        if (!sessions.ok || !actors.ok) {
          setLoadError(true);
        } else {
          setSources(sessions.result.sources);
          setMembers(actors.result.actors);
          onCurrentSourceDirectory?.(
            sessions.result.sources.find((source) => source.source_id === actor.external_session_id)
              ?.cwd || "",
          );
        }
        setLoading(false);
      })
      .catch(() => {
        if (!active) return;
        setLoadError(true);
        setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [
    actor.external_session_id,
    actor.id,
    groupId,
    onCurrentSourceDirectory,
    refreshKey,
    targetKind,
  ]);

  const selectedSource = sources.find((source) => source.source_id === sourceId);
  const ownerOf = (source: LocalSessionSource) =>
    members.find(
      (member) => member.id !== actor.id && member.external_session_id === source.source_id,
    );
  const selectedOwner = selectedSource ? ownerOf(selectedSource) : undefined;
  const choices = sources.map((source) => {
    const owner = ownerOf(source);
    const ownerName =
      owner?.title && owner.title !== owner.id ? `${owner.title} (${owner.id})` : owner?.id;
    return {
      value: source.source_id,
      label: `${source.runtime} · ${source.title || source.native_session_id}`,
      description: [
        source.cwd,
        owner ? t("runtimeSwitch.sourceInUse", { name: ownerName }) : "",
        !source.connected ? t("runtimeSwitch.sourceOffline") : "",
      ]
        .filter(Boolean)
        .join(" · "),
      disabled: Boolean(owner || !source.connected || !source.native_session_id),
    };
  });
  const availableProfiles = profiles.filter((profile) => !isWebModelRuntime(profile.runtime));
  const selectedProfile = availableProfiles.find(
    (profile) => actorProfileIdentityKey(profile) === profileKey,
  );
  const nativeAccessPaused = Boolean(actor.external_session_id) && actor.enabled === false;
  const linkedProfile = profiles.find(
    (profile) =>
      profile.id === actor.profile_id &&
      (profile.scope || "global") === (actor.profile_scope || "global") &&
      (profile.owner_id || "") === (actor.profile_owner || ""),
  );
  const generation = actorGenerationIdentity(actor);
  const expectedUpdatedAt = actor.updated_at;
  const detection = runtimeDetectionKey(
    runtime,
    runtimes.find((item) => item.name === runtime),
    runtimeDetectionStatus,
    runtimeMode,
  );
  // A checked runtime may use its recommended command; an explicit command
  // remains usable when automatic detection cannot establish availability.
  const runtimeReady = detection === "detected" || Boolean(command.trim());
  const scopeUnavailable =
    targetKind === "managed" &&
    Boolean(defaultScopeKey && !scopes.some((scope) => scope.scope_key === defaultScopeKey));
  const target =
    targetKind === "native"
      ? selectedSource &&
        selectedSource.connected &&
        !selectedOwner &&
        selectedSource.native_session_id &&
        selectedSource.source_id !== actor.external_session_id
        ? {
            kind: "native" as const,
            source_id: selectedSource.source_id,
            expected_native_session_id: selectedSource.native_session_id,
          }
        : undefined
      : configMode === "profile"
        ? selectedProfile && Number.isInteger(selectedProfile.revision)
          ? {
              kind: "managed" as const,
              patch: {
                runtime: String(selectedProfile.runtime),
                ...(defaultScopeKey !== (actor.default_scope_key || "")
                  ? { default_scope_key: defaultScopeKey }
                  : {}),
              },
              profile_id: selectedProfile.id,
              profile_scope: selectedProfile.scope || "global",
              profile_owner: selectedProfile.owner_id || undefined,
              expected_profile_revision: selectedProfile.revision,
              start: !(noStart || nativeAccessPaused),
            }
          : undefined
        : runtime && runtimeReady
          ? {
              kind: "managed" as const,
              patch: {
                runtime,
                runtime_mode: supportsAcpMode(runtime) ? runtimeMode : ("default" as RuntimeMode),
                command:
                  command.trim() ||
                  runtimes.find((item) => item.name === runtime)?.recommended_command ||
                  "",
                ...(defaultScopeKey !== (actor.default_scope_key || "")
                  ? { default_scope_key: defaultScopeKey }
                  : {}),
              },
              start: !(noStart || nativeAccessPaused),
            }
          : undefined;

  const submit = async () => {
    if (!target || generation === undefined || expectedUpdatedAt === undefined || busy || blocked)
      return;
    setBusy(true);
    setError("");
    try {
      const response = await api.switchActorRuntime(groupId, actor.id, {
        expected_actor_generation: generation,
        expected_actor_updated_at: expectedUpdatedAt,
        ...(actor.external_session_id ? { expected_source_id: actor.external_session_id } : {}),
        target,
      });
      if (!response.ok) {
        const code = response.error.code;
        const unknown = code === "actor_switch_unknown";
        if (unknown || code === "actor_switch_conflict") {
          setBlocked(true);
          onOutcomeUnknown?.();
        }
        setError(runtimeSwitchErrorMessage(response.error, (key) => t(key)));
      } else {
        setResult(response.result);
        onSwitched?.();
      }
    } catch {
      setBlocked(true);
      onOutcomeUnknown?.();
      setError(t("runtimeSwitch.unknown"));
    } finally {
      try {
        await refreshRuntimeSwitchGroup(groupId);
      } catch {
        setMessage(t("runtimeSwitch.refreshFailed"));
      }
      setBusy(false);
      setConfirming(false);
    }
  };

  return (
    <div className="space-y-4">
      {targetKind === "native" ? (
        <div className="space-y-2">
          <label className="block text-xs font-medium text-[var(--color-text-muted)]">
            {t("runtimeSwitch.source")}
          </label>
          <SelectCombobox
            value={sourceId}
            onChange={setSourceId}
            ariaLabel={t("runtimeSwitch.source")}
            placeholder={t("runtimeSwitch.chooseSource")}
            emptyText={t("runtimeSwitch.sourceEmpty")}
            searchable
            disabled={disabled || busy || loading || blocked || Boolean(result)}
            items={choices}
            className="w-full min-h-11 rounded-xl border px-4 py-2.5 text-sm glass-input text-[var(--color-text-primary)]"
            contentClassName="[&_[role=option]_.truncate]:whitespace-normal"
          />
          <p className="text-xs text-[var(--color-text-muted)]">{t("runtimeSwitch.sourceHint")}</p>
          {loadError ? (
            <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
              {t("runtimeSwitch.sourceLoadFailed")}
            </p>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={loading || busy || disabled}
            onClick={() => setRefreshKey((value) => value + 1)}
          >
            {t("runtimeSwitch.refreshSources")}
          </Button>
        </div>
      ) : (
        <div className="space-y-4">
          <RuntimeConfigurationModePicker
            value={configMode}
            disabled={disabled || busy || Boolean(result)}
            onChange={(mode) => {
              setConfigMode(mode);
              if (mode === "profile") void onRequestProfiles?.();
            }}
          />
          {configMode === "profile" ? (
            <RuntimeProfilePicker
              value={profileKey}
              profiles={availableProfiles}
              busy={profilesBusy}
              disabled={disabled || busy || Boolean(result)}
              onChange={setProfileKey}
            />
          ) : (
            <div className="space-y-3">
              <RuntimeSelector
                value={runtime}
                runtimeMode={runtimeMode}
                disabled={disabled || busy || Boolean(result)}
                allowedRuntimes={MANAGED_RUNTIMES}
                onChange={(value) => {
                  const next = value as SupportedRuntime;
                  setRuntime(next);
                  setRuntimeMode("default");
                  setCommand(
                    runtimes.find((item) => item.name === next)?.recommended_command || "",
                  );
                }}
                ariaLabel={t("runtimeSwitch.targetRuntime")}
              />
              {supportsAcpMode(runtime) ? (
                <AcpRuntimeMode
                  runtime={runtime}
                  value={runtimeMode}
                  onChange={setRuntimeMode}
                  disabled={disabled || busy}
                />
              ) : null}
              <label className="block space-y-2 text-xs font-medium text-[var(--color-text-muted)]">
                <span>{t("runtimeSwitch.command")}</span>
                <Input
                  value={command}
                  onChange={(event) => setCommand(event.target.value)}
                  placeholder={
                    runtimes.find((item) => item.name === runtime)?.recommended_command || ""
                  }
                  disabled={disabled || busy || Boolean(result)}
                />
              </label>
            </div>
          )}
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("runtimeSwitch.newSecrets")}
          </p>
          <ActorWorkingDirectory
            groupId={groupId}
            scopes={scopes}
            activeScopeKey={activeScopeKey}
            value={defaultScopeKey}
            onChange={setDefaultScopeKey}
            onBusyChange={setDirectoryBusy}
            disabled={disabled || busy || Boolean(result)}
            hint={t("workingDirectoryEditHint")}
          />
        </div>
      )}
      {generation === undefined || expectedUpdatedAt === undefined ? (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {t("runtimeSwitch.identityMissing")}
        </p>
      ) : null}
      {message ? (
        <p role="status" className="text-xs text-[var(--color-text-secondary)]">
          {message}
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {error}
        </p>
      ) : null}
      {result ? (
        <div role="status" className="space-y-1 text-xs text-[var(--color-text-secondary)]">
          <p>{t("runtimeSwitch.completed")}</p>
          {result.completion === "pending" ? <p>{t("runtimeSwitch.pending")}</p> : null}
          {result.start === "error" ? (
            <p>{runtimeSwitchStartError(result.start_error, (key) => t(key))}</p>
          ) : null}
          {result.start === "pending" ? <p>{t("runtimeSwitch.startPending")}</p> : null}
          {result.start === "stopped" ? <p>{t("runtimeSwitch.stopped")}</p> : null}
          {result.access_paused ? <p>{t("runtimeSwitch.accessPaused")}</p> : null}
        </div>
      ) : (
        <Button
          type="button"
          disabled={
            disabled ||
            loading ||
            busy ||
            directoryBusy ||
            scopeUnavailable ||
            blocked ||
            !target ||
            generation === undefined ||
            expectedUpdatedAt === undefined
          }
          onClick={() => {
            setError("");
            setMessage("");
            if (dirty) {
              setError(t("runtimeSwitch.dirty"));
              return;
            }
            setConfirming(true);
          }}
        >
          {t("runtimeSwitch.switch")}
        </Button>
      )}
      {confirming
        ? createPortal(
            <RuntimeSwitchConfirm
              isDark={isDark}
              actor={actor}
              targetLabel={
                targetKind === "native"
                  ? [
                      selectedSource?.runtime,
                      selectedSource?.title || selectedSource?.native_session_id,
                    ]
                      .filter(Boolean)
                      .join(" · ")
                  : configMode === "profile"
                    ? selectedProfile?.name?.trim() || selectedProfile?.id || runtime
                    : runtime
              }
              targetDirectory={
                targetKind === "native"
                  ? selectedSource?.cwd
                  : scopes.find((scope) => scope.scope_key === (defaultScopeKey || activeScopeKey))
                      ?.url
              }
              targetKind={targetKind}
              noStart={noStart || nativeAccessPaused}
              startLocked={nativeAccessPaused}
              onNoStart={setNoStart}
              busy={busy || profileBusy}
              confirmDisabled={profileSaveFailed}
              message={message}
              onCancel={() => {
                setConfirming(false);
                setProfileSaveFailed(false);
                setMessage("");
              }}
              onConfirm={() => void submit()}
              onSaveProfile={
                onSaveCurrentProfile && !actor.profile_id
                  ? async () => {
                      setProfileBusy(true);
                      try {
                        const saved = await onSaveCurrentProfile();
                        setProfileSaveFailed(!saved);
                        setMessage(
                          t(
                            saved
                              ? "runtimeSwitch.profileSaved"
                              : "runtimeSwitch.profileSaveFailed",
                          ),
                        );
                      } catch {
                        setProfileSaveFailed(true);
                        setMessage(t("runtimeSwitch.profileSaveFailed"));
                      } finally {
                        setProfileBusy(false);
                      }
                    }
                  : undefined
              }
              linkedProfileName={linkedProfile?.name || actor.profile_id}
            />,
            document.body,
          )
        : null}
    </div>
  );
}

export function RuntimeSwitchConfirm({
  isDark,
  actor,
  targetLabel,
  targetDirectory,
  targetKind,
  noStart,
  onNoStart,
  busy,
  message,
  onCancel,
  onConfirm,
  onSaveProfile,
  profileToCustom = false,
  keepCustomSecrets = false,
  secretKeys = [],
  confirmDisabled = false,
  linkedProfileName,
  startLocked = false,
}: {
  isDark: boolean;
  actor: Actor;
  targetLabel: string;
  targetDirectory?: string;
  targetKind: "native" | "managed";
  noStart: boolean;
  onNoStart: (value: boolean) => void;
  busy: boolean;
  message: string;
  onCancel: () => void;
  onConfirm: () => void;
  onSaveProfile?: () => Promise<void>;
  profileToCustom?: boolean;
  keepCustomSecrets?: boolean;
  secretKeys?: string[];
  confirmDisabled?: boolean;
  linkedProfileName?: string;
  startLocked?: boolean;
}) {
  const { t } = useTranslation("actors");
  const { modalRef } = useModalA11y(true, () => {
    if (!busy) onCancel();
  });
  const leavingNative = Boolean(actor.external_session_id);
  return (
    <ModalFrame
      isDark={isDark}
      onClose={onCancel}
      closeDisabled={busy}
      modalRef={modalRef}
      titleId="runtime-switch-confirm-title"
      title={t("runtimeSwitch.confirmTitle")}
      closeAriaLabel={t("common:close")}
      panelClassName="w-full sm:max-w-xl sm:max-h-[80dvh]"
      surface="solid"
      footerActions={
        <div className="flex flex-wrap items-center justify-end gap-2">
          {!leavingNative && targetKind === "native" && onSaveProfile ? (
            <Button
              type="button"
              variant="outline"
              className="mr-auto"
              disabled={busy}
              onClick={() => void onSaveProfile()}
            >
              {t("runtimeSwitch.saveProfile")}
            </Button>
          ) : null}
          <Button type="button" variant="outline" disabled={busy} onClick={onCancel}>
            {t("common:cancel")}
          </Button>
          <Button type="button" disabled={busy || confirmDisabled} onClick={onConfirm}>
            {t(busy ? "runtimeSwitch.switching" : "runtimeSwitch.confirm")}
          </Button>
        </div>
      }
    >
      <div className="min-h-0 space-y-3 overflow-auto p-4 text-sm sm:p-5">
        <p>{t("runtimeSwitch.preserve")}</p>
        <p className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2">
          {t("runtimeSwitch.transition", {
            from: `${t(leavingNative ? "runtimeSwitch.native" : "runtimeSwitch.managed")} · ${actor.runtime}`,
            to: `${t(targetKind === "native" ? "runtimeSwitch.native" : "runtimeSwitch.managed")} · ${targetLabel}`,
          })}
          {targetDirectory ? (
            <span className="mt-1 block break-all text-xs text-[var(--color-text-secondary)]">
              {t("workingDirectory")}: {targetDirectory}
            </span>
          ) : null}
        </p>
        <p
          className="rounded-xl border border-amber-500/20 bg-amber-500/5 px-3 py-2 text-xs leading-5 text-amber-700 dark:text-amber-300"
          data-status-tone="warning"
        >
          {joinRuntimeSentences([
            t(leavingNative ? "runtimeSwitch.oldNative" : "runtimeSwitch.oldManaged"),
            !leavingNative && targetKind === "native" && actor.profile_id
              ? t("runtimeSwitch.unlinkProfile", { name: linkedProfileName || actor.profile_id })
              : "",
            !leavingNative && targetKind === "native" && !actor.profile_id
              ? t("runtimeSwitch.clearCustomManaged")
              : "",
            !leavingNative && targetKind === "native" && !actor.profile_id
              ? t("runtimeSwitch.profileHint")
              : "",
            profileToCustom ? t("runtimeSwitch.clearDormantSecrets") : "",
            keepCustomSecrets ? t("runtimeSwitch.keepSecrets") : "",
            keepCustomSecrets && secretKeys.length
              ? t("runtimeSwitch.secretKeys", { keys: secretKeys.join(", ") })
              : "",
          ])}
        </p>
        {actor.enabled === false && (leavingNative || targetKind === "native") ? (
          <p className="text-xs text-[var(--color-text-secondary)]">
            {t(
              targetKind === "managed"
                ? "runtimeSwitch.pausedNativeToManaged"
                : "runtimeSwitch.pausedNativeTarget",
            )}
          </p>
        ) : null}
        {targetKind === "managed" ? (
          <label className="flex items-start gap-2 text-xs">
            <input
              type="checkbox"
              checked={noStart}
              onChange={(event) => onNoStart(event.target.checked)}
              disabled={busy || startLocked}
            />
            {t("runtimeSwitch.noStart")}
          </label>
        ) : null}
        {message ? (
          <p role="status" className="text-xs">
            {message}
          </p>
        ) : null}
      </div>
    </ModalFrame>
  );
}
