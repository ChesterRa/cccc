import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CodexVoiceAnalystPane,
  CodexVoiceAnalystSummaryBar,
  CodexVoiceConversationPane,
} from "../../features/codexVoice/CodexVoiceConsolePanes";
import { CodexVoiceSplitLayout } from "../../features/codexVoice/CodexVoiceSplitLayout";
import { useModalStore } from "../../stores";
import { CodexVoiceMessageSources } from "../../features/codexVoice/CodexVoiceMessageSources";
import { voicePhaseDotClass } from "../../features/codexVoice/codexVoicePhase";
import {
  codexVoiceAnalystReadinessProblem,
  codexVoiceCallReadinessProblem,
  codexVoiceCallStatus,
} from "../../features/codexVoice/codexVoiceControllerText";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { useModalA11y } from "../../hooks/useModalA11y";
import {
  HeadphonesIcon,
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

type Props = {
  isOpen: boolean;
  isDark: boolean;
  isSmallScreen: boolean;
  controller: CodexVoiceSessionController;
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

export function CodexVoiceAnalystModal({
  isOpen,
  isDark,
  isSmallScreen,
  controller,
  onClose,
  onOpenSource,
}: Props) {
  const { t } = useTranslation("modals");
  const [analystExpanded, setAnalystExpanded] = useState(false);
  const openSettings = useModalStore((state) => state.openCodexVoiceSettings);
  const [mobilePane, setMobilePane] = useState<MobilePane>("conversation");
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [splitLayout, setSplitLayout] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return !isSmallScreen;
    }
    return window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY).matches;
  });
  const analyst = controller.analyst;
  const callStatus = codexVoiceCallStatus(controller);
  const phaseLabel = t(callStatus.labelKey);
  const analystPhase = analyst ? t(`codexVoiceAnalystPhase.${analyst.phase}`) : "";
  const analystVisible = isOpen && (splitLayout ? analystExpanded : mobilePane === "analyst");
  const terminalVisible = analystVisible && Boolean(analyst?.structured || analyst?.tui_ready);
  const callProblem = codexVoiceCallReadinessProblem(t, controller.readiness);
  const analystProblem = codexVoiceAnalystReadinessProblem(t, controller.readiness);
  const callBlocked = callStatus.blocked;
  const showReadiness =
    callBlocked &&
    (!controller.error ||
      (callProblem !== controller.error && analystProblem !== controller.error));
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

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY);
    const update = () => setSplitLayout(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  const startVoice = () => {
    void controller.start();
  };

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="codex-voice-analyst-title"
      closeAriaLabel={t("codexVoiceMinimize")}
      closeIcon={<ChevronDownIcon size={18} aria-hidden="true" />}
      headerClassName="!gap-2 !px-3 !py-3 sm:!px-5"
      panelClassName="h-full w-full overflow-hidden sm:h-[min(820px,92vh)] sm:w-[min(1180px,97vw)]"
      modalRef={modalRef}
      title={
        <div className="flex min-w-0 items-center gap-3">
          <div className="hidden h-10 w-10 flex-none sm:flex items-center justify-center rounded-2xl bg-[var(--glass-tab-bg-active)] text-[var(--color-accent-primary)]">
            <HeadphonesIcon size={20} aria-hidden="true" />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <h2 className="truncate text-base font-semibold text-[var(--color-text-primary)]">
                {t("codexVoiceTitle")}
              </h2>
              <span className="hidden rounded-full border border-[var(--glass-border-subtle)] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--color-text-muted)] sm:inline-flex">
                {t("codexVoiceExperimental")}
              </span>
            </div>
            <div className="mt-0.5 flex items-center gap-2 text-xs text-[var(--color-text-muted)]">
              <span
                className={`h-2 w-2 flex-none rounded-full ${
                  callBlocked
                    ? "bg-amber-500"
                    : voicePhaseDotClass(controller.phase, controller.externalCall)
                }`}
                aria-hidden="true"
              />
              <span
                className={`truncate ${callBlocked ? "text-amber-700 dark:text-amber-300" : ""}`}
              >
                {phaseLabel}
              </span>
            </div>
            <span className="sr-only" aria-live="polite" aria-atomic="true">
              {isOpen ? callAnnouncement : ""}
            </span>
          </div>
        </div>
      }
      headerActions={
        <>
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={openSettings}
            label={t("codexVoiceSettings")}
          >
            <SettingsIcon size={17} />
          </IconButton>
          {controller.isEngaged ? (
            <>
              {controller.owned ? (
                <IconButton
                  type="button"
                  variant="ghost"
                  size="sm"
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
                aria-label={
                  controller.externalCall ? t("codexVoiceStopExisting") : t("codexVoiceStop")
                }
                onClick={() => void controller.disconnect()}
                disabled={controller.phase === "stopping"}
                className="text-rose-500"
              >
                <StopIcon size={15} />
                <span className="hidden sm:inline">
                  {controller.externalCall ? t("codexVoiceStopExisting") : t("codexVoiceStop")}
                </span>
              </Button>
            </>
          ) : (
            <Button
              type="button"
              size="sm"
              aria-label={t("codexVoiceStart")}
              onClick={startVoice}
              disabled={controller.checking || controller.isStarting}
            >
              <VoiceWaveformIcon size={15} />
              <span className="hidden sm:inline">
                {controller.isStarting ? t("codexVoiceStarting") : t("codexVoiceStart")}
              </span>
            </Button>
          )}
        </>
      }
    >
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {controller.error ? (
          <div
            className="flex flex-none items-center justify-between gap-3 border-b border-rose-400/25 bg-rose-500/8 px-5 py-2.5 text-sm text-rose-500 sm:px-6"
            role="alert"
          >
            <span>{controller.error}</span>
            <Button type="button" variant="ghost" size="sm" onClick={controller.clearError}>
              {t("codexVoiceDismissError")}
            </Button>
          </div>
        ) : null}

        {controller.playbackBlocked && controller.owned ? (
          <div className="flex flex-none items-center justify-between gap-3 border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-sm text-[var(--color-text-secondary)] sm:px-6">
            <span>{t("codexVoicePlaybackBlocked")}</span>
            <Button type="button" variant="ghost" size="sm" onClick={controller.resumeAudio}>
              <VolumeIcon size={15} />
              {t("codexVoiceResumeAudio")}
            </Button>
          </div>
        ) : null}

        {showReadiness ? (
          <div
            data-codex-voice-readiness
            className="flex flex-none flex-wrap items-start gap-x-4 gap-y-2 border-b border-amber-400/25 bg-amber-400/8 px-4 py-2.5 text-sm sm:px-6"
          >
            <dl className="min-w-0 flex-1 space-y-1 leading-6">
              {[
                ["codexVoiceReadinessCall", callProblem],
                ["codexVoiceAnalystTitle", analystProblem],
              ].map(([label, problem]) => (
                <div key={label} className="flex min-w-0 gap-2">
                  <dt className="flex-none font-semibold text-[var(--color-text-primary)]">
                    {t(label)}
                  </dt>
                  <dd
                    className={`min-w-0 break-words ${
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
              variant="secondary"
              size="sm"
              className="flex-none"
              onClick={openSettings}
            >
              {t("codexVoiceOpenSettings")}
            </Button>
          </div>
        ) : null}

        {controller.notificationPaused && controller.isEngaged ? (
          <p
            role="status"
            className="border-b border-amber-400/25 bg-amber-400/8 px-5 py-3 text-sm text-amber-700 dark:text-amber-300"
          >
            {t("voicePreferences.paused")}
          </p>
        ) : null}
        <div className="flex min-h-0 flex-1 flex-col" data-codex-voice-console="true">
          <div className="flex flex-none border-b border-[var(--glass-border-subtle)] lg:hidden">
            {(["conversation", "analyst"] as const).map((pane) => {
              const label = t(
                pane === "conversation" ? "codexVoiceConversation" : "codexVoiceAnalystTitle",
              );
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
              <CodexVoiceConversationPane
                controller={controller}
                visible={splitLayout || mobilePane === "conversation"}
                analystExpanded={analystExpanded}
                onToggleAnalyst={() => setAnalystExpanded((expanded) => !expanded)}
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
                    />
                  )
                }
              >
                <CodexVoiceMessageSources
                  active={isOpen}
                  outputStatus={controller.owned ? controller.outputStatus : undefined}
                  onOpenSource={
                    onOpenSource
                      ? (groupId, eventId) => {
                          onClose();
                          onOpenSource(groupId, eventId);
                        }
                      : undefined
                  }
                />
              </CodexVoiceConversationPane>
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
    </ModalFrame>
  );
}
