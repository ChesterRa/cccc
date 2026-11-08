import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CodexVoiceAnalystPane,
  CodexVoiceAnalystSummaryBar,
} from "../../features/codexVoice/CodexVoiceConsolePanes";
import { CodexVoiceSplitLayout } from "../../features/codexVoice/CodexVoiceSplitLayout";
import { useModalStore } from "../../stores";
import { CodexVoiceMessageSources } from "../../features/codexVoice/CodexVoiceMessageSources";
import {
  voicePhaseDotState,
  voicePhaseHasLocalActivity,
} from "../../features/codexVoice/codexVoicePhase";
import {
  codexVoiceAnalystReadinessProblem,
  codexVoiceCallReadinessProblem,
  codexVoiceCallStatus,
  codexVoiceDisplayIdentity,
} from "../../features/codexVoice/codexVoiceControllerText";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { ChetHead } from "../../features/chet/ChetHead";
import { ChetConversationPane, ChetStopWorkButton } from "../../features/chet/ChetConversationPane";
import type { ChetConversation } from "../../features/chet/useChetConversation";
import type { ProcessRecoveryOwner } from "../../services/api/processRecovery";
import { useModalA11y } from "../../hooks/useModalA11y";
import {
  ChevronDownIcon,
  MicrophoneIcon,
  MicrophoneOffIcon,
  SettingsIcon,
  StopIcon,
  VoiceWaveformIcon,
  VolumeIcon,
} from "../Icons";
import { Button } from "../ui/button";
import { IconButton } from "../ui/icon-button";
import { ModalFrame } from "./ModalFrame";
import { useChatObscuringView } from "../../hooks/useChatObscuringView";
import { ProcessRecoveryDialog } from "./ProcessRecoveryDialog";
import { VoiceProviderSelect } from "../../features/codexVoice/VoiceProviderSelect";
import { SpeakingVoiceSelect } from "../../features/codexVoice/SpeakingVoiceSelect";

type Props = {
  isOpen: boolean;
  isDark: boolean;
  isSmallScreen: boolean;
  controller: CodexVoiceSessionController;
  chet: ChetConversation;
  onClose: () => void;
  onOpenSource?: (groupId: string, eventId: string) => void;
};

type MobilePane = "conversation" | "analyst";
const VOICE_CONSOLE_SPLIT_MEDIA_QUERY = "(min-width: 1024px)";
const IN_CALL_PHASES = new Set(["listening", "responding", "speaking", "analysing"]);

function analystResultSignature(analyst: CodexVoiceSessionController["analyst"]): string {
  if (!analyst) return "";
  const completed = (analyst.manual_tasks || []).filter((task) => task.result).length;
  return `${analyst.generation}\n${completed}\n${analyst.last_result}`;
}

/**
 * Chet, the global assistant: typed and spoken requests in one conversation, with the
 * shared Analyst beside it. Closing only hides the dialog; work and calls keep running.
 */
export function CodexVoiceAnalystModal({
  isOpen,
  isDark,
  isSmallScreen,
  controller,
  chet,
  onClose,
  onOpenSource,
}: Props) {
  const { t } = useTranslation("modals");
  const [analystExpanded, setAnalystExpanded] = useState(false);
  const [recoveryOwners, setRecoveryOwners] = useState<ProcessRecoveryOwner[] | null>(null);
  const openSettings = useModalStore((state) => state.openCodexVoiceSettings);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const [mobilePane, setMobilePane] = useState<MobilePane>("conversation");
  const { modalRef } = useModalA11y(isOpen, onClose);
  useChatObscuringView(isOpen);
  const [splitLayout, setSplitLayout] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return !isSmallScreen;
    }
    return window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY).matches;
  });
  const analyst = controller.analyst;
  const callStatus = codexVoiceCallStatus(controller);
  const phaseLabel = t(callStatus.labelKey);
  const identity = codexVoiceDisplayIdentity(controller);
  const providerLabel = t(`codexVoiceProviders.${identity.provider}`);
  const analystPhase = analyst ? t(`codexVoiceAnalystPhase.${analyst.phase}`) : "";
  const analystVisible = isOpen && (splitLayout ? analystExpanded : mobilePane === "analyst");
  const terminalVisible = analystVisible && Boolean(analyst?.structured || analyst?.tui_ready);
  const callProblem = codexVoiceCallReadinessProblem(t, controller.readiness);
  const analystProblem = codexVoiceAnalystReadinessProblem(t, controller.readiness);
  const callBlocked = callStatus.blocked;
  // A call that is not ready yet is noted beside the voice controls; the full readiness
  // list is for an Analyst problem, which stops text as well.
  const showReadiness =
    callBlocked &&
    Boolean(analystProblem) &&
    (!controller.error ||
      (callProblem !== controller.error && analystProblem !== controller.error));
  const openProblemSettings = () =>
    openSettingsTarget({
      scope: "global",
      tab: "voice",
      voiceSection: "realtime",
      voiceAnalyst: !!analystProblem,
    });
  const attentionCount = analyst?.permissions?.length || 0;
  const analystErrorText =
    analyst?.phase === "needs_attention" ? analyst.last_error || controller.analystWarning : "";
  const resultSignature = analystResultSignature(analyst);
  const [seenResultSignature, setSeenResultSignature] = useState(resultSignature);
  if (analystVisible && seenResultSignature !== resultSignature) {
    setSeenResultSignature(resultSignature);
  }
  const hasNewResult =
    !analystVisible &&
    Boolean(analyst?.last_result || analyst?.manual_tasks?.length) &&
    resultSignature !== seenResultSignature;
  const analystNeedsLook =
    Boolean(analystProblem) || attentionCount > 0 || Boolean(analystErrorText) || hasNewResult;
  const callAnnouncement = controller.externalCall
    ? phaseLabel
    : IN_CALL_PHASES.has(controller.phase)
      ? t("codexVoiceInCall")
      : controller.phase === "failed" || controller.checking
        ? ""
        : phaseLabel;
  const touchTarget = "max-sm:min-h-11 pointer-coarse:min-h-11";
  // A message opens in its Group, behind this dialog, so the dialog steps aside.
  const openSource = onOpenSource
    ? (groupId: string, eventId: string) => {
        onClose();
        onOpenSource(groupId, eventId);
      }
    : undefined;

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY);
    const update = () => setSplitLayout(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  const dotState = callBlocked
    ? "attention"
    : voicePhaseDotState(controller.phase, controller.externalCall);
  const callDot = (
    <span
      data-status-motion={
        !callBlocked &&
        voicePhaseHasLocalActivity(controller.phase, {
          microphoneMuted: controller.microphoneMuted,
          externalCall: controller.externalCall,
          playbackBlocked: controller.playbackBlocked,
        })
          ? "working"
          : undefined
      }
      data-status-dot={dotState}
      className={`h-2 w-2 flex-none ${!callBlocked && dotState === "attention" ? "animate-pulse" : ""}`}
      aria-hidden="true"
    />
  );

  // Before a call: what the next call uses. During one: who is speaking and how it goes.
  const voiceStatus = controller.isEngaged ? (
    <div className="flex min-w-0 items-center gap-1.5 text-xs text-[var(--color-text-muted)]">
      {callDot}
      <span
        className="min-w-0 truncate"
        title={[providerLabel, identity.voice, phaseLabel].filter(Boolean).join(" · ")}
      >
        {[providerLabel, identity.voice, phaseLabel].filter(Boolean).join(" · ")}
      </span>
    </div>
  ) : (
    <>
      {/* Phones keep these choices in Settings, so the composer stays one row. */}
      <VoiceProviderSelect
        controller={controller}
        className={`min-h-9 max-w-full text-xs max-sm:hidden ${touchTarget}`}
      />
      <SpeakingVoiceSelect
        controller={controller}
        className={`min-h-9 max-w-full rounded-lg border border-[var(--color-border-secondary)] bg-[var(--color-bg-primary)] px-2 text-xs text-[var(--color-text-primary)] max-sm:hidden ${touchTarget}`}
      />
      {callBlocked ? (
        <button
          type="button"
          className="flex min-w-0 items-center gap-1.5 rounded text-left text-xs text-amber-700 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] dark:text-amber-300"
          data-status-tone="warning"
          title={callProblem || phaseLabel}
          onClick={openProblemSettings}
        >
          {callDot}
          <span className="truncate">{phaseLabel}</span>
        </button>
      ) : controller.checking ? (
        <span className="flex min-w-0 items-center gap-1.5 text-xs text-[var(--color-text-muted)]">
          {callDot}
          <span className="truncate">{phaseLabel}</span>
        </span>
      ) : null}
    </>
  );

  const voiceActions = controller.isEngaged ? (
    <>
      {controller.owned ? (
        <IconButton
          type="button"
          variant="ghost"
          size="sm"
          data-lcd-key
          className="max-sm:h-11 max-sm:w-11"
          onClick={controller.toggleMicrophone}
          disabled={controller.isStarting || controller.phase === "stopping"}
          label={controller.microphoneMuted ? t("codexVoiceUnmute") : t("codexVoiceMute")}
          aria-pressed={controller.microphoneMuted}
        >
          {controller.microphoneMuted ? (
            <MicrophoneOffIcon size={17} />
          ) : (
            <MicrophoneIcon size={17} />
          )}
        </IconButton>
      ) : null}
      <Button
        type="button"
        variant="secondary"
        size="sm"
        data-lcd-key
        onClick={() => void controller.disconnect()}
        disabled={controller.phase === "stopping"}
        className="text-rose-500 max-sm:min-h-11"
      >
        <StopIcon size={15} aria-hidden="true" />
        {controller.externalCall ? t("codexVoiceStopExisting") : t("codexVoiceStop")}
      </Button>
    </>
  ) : (
    <Button
      type="button"
      variant="secondary"
      size="sm"
      data-lcd-key
      className="max-sm:min-h-11"
      onClick={() => void controller.start()}
      disabled={controller.checking || controller.isStarting}
    >
      <VoiceWaveformIcon size={15} aria-hidden="true" />
      {controller.isStarting ? t("codexVoiceStarting") : t("codexVoiceStart")}
    </Button>
  );

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="codex-voice-analyst-title"
      closeAriaLabel={t("chet.minimize")}
      closeIcon={<ChevronDownIcon size={18} aria-hidden="true" />}
      headerClassName="!gap-2 !px-3 !py-2 sm:!px-4 [&>div:first-child]:pr-0 [&>div:last-child]:gap-1"
      panelClassName="h-full w-full overflow-hidden sm:h-[min(820px,92vh)] sm:w-[min(1180px,97vw)]"
      modalRef={modalRef}
      title={
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
          <ChetHead
            size={16}
            working={chet.working || controller.analystWorking}
            className="self-center"
          />
          <h2 className="text-sm font-semibold text-[var(--color-text-primary)]">Chet</h2>
          <span className="truncate text-xs text-[var(--color-text-muted)]">
            {t("chet.subtitle")}
          </span>
          <span className="sr-only" aria-live="polite" aria-atomic="true">
            {isOpen ? callAnnouncement : ""}
          </span>
        </div>
      }
      headerActions={
        <IconButton
          type="button"
          variant="ghost"
          size="sm"
          data-lcd-key
          className="max-sm:h-11 max-sm:w-11"
          onClick={openSettings}
          label={t("codexVoiceSettings")}
        >
          <SettingsIcon size={17} />
        </IconButton>
      }
    >
      <div className="relative flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto">
        {controller.refreshError && !controller.owned ? (
          <div
            data-codex-voice-status-read
            data-status-tone="warning"
            className="flex flex-none flex-wrap items-center justify-between gap-3 border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-sm text-amber-700 dark:text-amber-300 sm:px-6"
            role="status"
          >
            <span className="min-w-0 flex-1 break-words">{controller.refreshError}</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-lcd-key
              disabled={controller.checking}
              onClick={() => void controller.refresh()}
            >
              {t("common:retry")}
            </Button>
          </div>
        ) : null}
        {controller.error ? (
          <div
            data-status-tone="error"
            className="flex flex-none flex-wrap items-center justify-between gap-3 border-b border-rose-400/25 bg-rose-500/8 px-5 py-2.5 text-sm text-rose-500 sm:px-6"
            role="alert"
          >
            <span className="min-w-0 flex-1 break-words">{controller.error}</span>
            <div className="flex shrink-0 flex-wrap items-center gap-2">
              {callBlocked && analystProblem && !showReadiness && (
                <Button type="button" variant="secondary" size="sm" onClick={openProblemSettings}>
                  {t("codexVoiceOpenSettings")}
                </Button>
              )}
              {controller.recoveryOwners.length > 0 ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  data-lcd-key
                  onClick={() => setRecoveryOwners(controller.recoveryOwners)}
                >
                  {t("codexVoiceReviewRecoveryRecords")}
                </Button>
              ) : null}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-lcd-key
                onClick={controller.clearError}
              >
                {t("codexVoiceDismissError")}
              </Button>
            </div>
          </div>
        ) : null}

        {controller.playbackBlocked && controller.owned ? (
          <div
            data-status-tone="warning"
            className="flex flex-none items-center justify-between gap-3 border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-sm text-[var(--color-text-secondary)] sm:px-6"
          >
            <span>{t("codexVoicePlaybackBlocked")}</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-lcd-key
              onClick={controller.resumeAudio}
            >
              <VolumeIcon size={15} />
              {t("codexVoiceResumeAudio")}
            </Button>
          </div>
        ) : null}

        {showReadiness ? (
          <div
            data-codex-voice-readiness
            data-status-tone="warning"
            className="flex flex-none flex-wrap items-start gap-x-4 gap-y-2 border-b border-amber-400/25 bg-amber-400/8 px-4 py-2.5 text-sm sm:px-6"
          >
            <dl className="min-w-0 flex-1 basis-full space-y-1 leading-6 sm:basis-0">
              {[
                ["codexVoiceReadinessCall", callProblem],
                ["chet.runtime", analystProblem],
              ].map(([label, problem]) => (
                <div key={label} className="block min-w-0 sm:flex sm:gap-2">
                  <dt className="mr-2 inline font-semibold text-[var(--color-text-primary)] sm:mr-0 sm:block sm:flex-none">
                    {t(label)}
                  </dt>
                  <dd
                    className={`inline min-w-0 break-words sm:block ${
                      problem
                        ? "text-amber-700 dark:text-amber-300"
                        : "text-[var(--color-text-muted)]"
                    }`}
                  >
                    {problem || t("codexVoiceReadinessOk")}
                  </dd>
                </div>
              ))}
            </dl>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-lcd-key
              disabled={controller.checking}
              onClick={() => void controller.refresh()}
            >
              {t("settings:voiceSettings.refresh")}
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="flex-none"
              onClick={openProblemSettings}
            >
              {t("codexVoiceOpenSettings")}
            </Button>
          </div>
        ) : null}

        {controller.notificationPaused && controller.isEngaged ? (
          <p
            role="status"
            data-status-tone="warning"
            className="border-b border-amber-400/25 bg-amber-400/8 px-5 py-3 text-sm text-amber-700 dark:text-amber-300"
          >
            {t("voicePreferences.paused")}
          </p>
        ) : null}
        <div className="flex min-h-0 flex-1 flex-col" data-codex-voice-console="true">
          <div
            data-voice-pane-switch
            className="flex flex-none border-b border-[var(--glass-border-subtle)] lg:hidden"
          >
            {(["conversation", "analyst"] as const).map((pane) => {
              const label = t(pane === "conversation" ? "chet.conversation" : "chet.activity");
              const marked = pane === "analyst" && analystNeedsLook && mobilePane !== "analyst";
              return (
                <button
                  key={pane}
                  type="button"
                  aria-pressed={mobilePane === pane}
                  aria-label={marked ? `${label} · ${t("codexVoiceAnalystHasUpdates")}` : undefined}
                  className={`inline-flex min-h-11 flex-1 items-center justify-center gap-1.5 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
                    mobilePane === pane
                      ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]"
                      : "border-transparent text-[var(--color-text-muted)]"
                  }`}
                  onClick={() => setMobilePane(pane)}
                >
                  {label}
                  {marked ? (
                    <span
                      data-codex-voice-analyst-dot
                      data-status-dot="attention"
                      className="h-1.5 w-1.5 rounded-full bg-amber-500"
                      aria-hidden="true"
                    />
                  ) : null}
                </button>
              );
            })}
          </div>

          <CodexVoiceSplitLayout
            enabled={splitLayout && analystExpanded}
            active={isOpen}
            conversation={
              <ChetConversationPane
                controller={controller}
                chet={chet}
                isDark={isDark}
                visible={splitLayout || mobilePane === "conversation"}
                analystExpanded={analystExpanded}
                onToggleAnalyst={() => setAnalystExpanded((expanded) => !expanded)}
                onReviewRecovery={setRecoveryOwners}
                onOpenGroupMessage={openSource}
                voiceStatus={voiceStatus}
                voiceActions={voiceActions}
                analystBar={
                  analystExpanded ? null : (
                    <CodexVoiceAnalystSummaryBar
                      controller={controller}
                      analystPhase={analystPhase}
                      notReady={Boolean(analystProblem)}
                      attentionCount={attentionCount}
                      errorText={analystErrorText}
                      hasNewResult={hasNewResult}
                      onExpand={() => setAnalystExpanded(true)}
                      actions={
                        controller.analystWorking ? (
                          <ChetStopWorkButton controller={controller} />
                        ) : null
                      }
                    />
                  )
                }
                sources={
                  <CodexVoiceMessageSources
                    active={isOpen}
                    outputStatus={controller.owned ? controller.outputStatus : undefined}
                    onOpenSource={openSource}
                  />
                }
              />
            }
            analyst={
              <div className={splitLayout && !analystExpanded ? "hidden" : "contents"}>
                <CodexVoiceAnalystPane
                  controller={controller}
                  analystPhase={analystPhase}
                  visible={mobilePane === "analyst"}
                  terminalVisible={terminalVisible}
                  isDark={isDark}
                  notReady={Boolean(analystProblem)}
                />
              </div>
            }
          />
        </div>
      </div>
      <ProcessRecoveryDialog
        isOpen={recoveryOwners !== null}
        owners={recoveryOwners || []}
        isDark={isDark}
        onClose={() => setRecoveryOwners(null)}
      />
    </ModalFrame>
  );
}
