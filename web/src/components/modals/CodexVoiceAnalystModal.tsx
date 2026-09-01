import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CodexVoiceAudioSettings } from "../../features/codexVoice/CodexVoiceAudioSettings";
import {
  CodexVoiceAnalystPane,
  CodexVoiceConversationPane,
} from "../../features/codexVoice/CodexVoiceConsolePanes";
import { voicePhaseDotClass } from "../../features/codexVoice/codexVoicePhase";
import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";
import { useModalA11y } from "../../hooks/useModalA11y";
import {
  HeadphonesIcon,
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
  selectedGroupId: string;
  controller: CodexVoiceSessionController;
  onClose: () => void;
};

type MobilePane = "conversation" | "analyst";
const VOICE_CONSOLE_SPLIT_MEDIA_QUERY = "(min-width: 1024px)";

function codexVoiceTerminalShouldConnect({
  isOpen,
  tuiReady,
  splitLayout,
  mobilePane,
}: {
  isOpen: boolean;
  tuiReady: boolean;
  splitLayout: boolean;
  mobilePane: MobilePane;
}): boolean {
  return isOpen && tuiReady && (splitLayout || mobilePane === "analyst");
}

export function CodexVoiceAnalystModal({
  isOpen,
  isDark,
  isSmallScreen,
  selectedGroupId,
  controller,
  onClose,
}: Props) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mobilePane, setMobilePane] = useState<MobilePane>("conversation");
  const [splitLayout, setSplitLayout] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return !isSmallScreen;
    }
    return window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY).matches;
  });
  const analyst = controller.analyst;
  const phaseLabel = controller.externalCall
    ? t("codexVoiceActiveElsewhere")
    : controller.checking && !controller.isEngaged
      ? t("codexVoiceChecking")
      : t(`codexVoicePhase.${controller.phase}`);
  const analystPhase = analyst ? t(`codexVoiceAnalystPhase.${analyst.phase}`) : "";
  const terminalVisible = codexVoiceTerminalShouldConnect({
    isOpen,
    tuiReady: Boolean(analyst?.tui_ready),
    splitLayout,
    mobilePane,
  });
  const readinessProblem = !controller.readiness
    ? ""
    : !controller.readiness.codex_cli_available
      ? t("codexVoiceCodexCliMissing")
      : !controller.readiness.codex_credentials_available
        ? t("codexVoiceCodexLoginRequired")
        : "";
  const startupProblem =
    readinessProblem || (!selectedGroupId && !analyst ? t("codexVoiceRepositoryRequired") : "");

  useEffect(() => {
    if (!isOpen) {
      setSettingsOpen(false);
      setMobilePane("conversation");
    }
  }, [isOpen]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia(VOICE_CONSOLE_SPLIT_MEDIA_QUERY);
    const update = () => setSplitLayout(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  const startVoice = () => {
    const launchGroupId = controller.analyst?.group_id || selectedGroupId;
    if (!launchGroupId) return;
    void controller.start(launchGroupId);
  };

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="codex-voice-analyst-title"
      closeAriaLabel={t("codexVoiceMinimize")}
      panelClassName="h-full w-full overflow-hidden sm:h-[min(820px,92vh)] sm:w-[min(1180px,97vw)]"
      modalRef={modalRef}
      title={
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-10 w-10 flex-none items-center justify-center rounded-2xl bg-[var(--glass-tab-bg-active)] text-[var(--color-accent-primary)]">
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
                className={`h-2 w-2 rounded-full ${voicePhaseDotClass(
                  controller.phase,
                  controller.externalCall,
                )}`}
                aria-hidden="true"
              />
              <span className="truncate">{phaseLabel}</span>
            </div>
          </div>
        </div>
      }
      headerActions={
        <>
          <IconButton
            type="button"
            variant={settingsOpen ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setSettingsOpen((open) => !open)}
            label={t("codexVoiceAudioSettings")}
            aria-expanded={settingsOpen}
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
              onClick={startVoice}
              disabled={
                (!selectedGroupId && !controller.analyst) ||
                controller.checking ||
                controller.isStarting
              }
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
      {settingsOpen ? (
        <CodexVoiceAudioSettings active={isOpen && settingsOpen} controller={controller} />
      ) : null}

      <div className="flex min-h-0 flex-1 flex-col">
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

        {!controller.isEngaged && startupProblem ? (
          <div className="flex-none border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-sm text-amber-700 dark:text-amber-300 sm:px-6">
            {startupProblem}
          </div>
        ) : null}

        <div className="flex flex-none border-b border-[var(--glass-border-subtle)] lg:hidden">
          {(["conversation", "analyst"] as const).map((pane) => (
            <button
              key={pane}
              type="button"
              className={`flex-1 border-b-2 px-4 py-2.5 text-xs font-medium transition-colors ${
                mobilePane === pane
                  ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]"
                  : "border-transparent text-[var(--color-text-muted)]"
              }`}
              onClick={() => setMobilePane(pane)}
            >
              {t(pane === "conversation" ? "codexVoiceConversation" : "codexVoiceAnalystTitle")}
            </button>
          ))}
        </div>

        <div className="grid min-h-0 flex-1 lg:grid-cols-[minmax(280px,0.72fr)_minmax(0,1.55fr)]">
          <CodexVoiceConversationPane
            controller={controller}
            phaseLabel={phaseLabel}
            visible={mobilePane === "conversation"}
          />
          <CodexVoiceAnalystPane
            controller={controller}
            analystPhase={analystPhase}
            visible={mobilePane === "analyst"}
            terminalVisible={terminalVisible}
          />
        </div>
      </div>
    </ModalFrame>
  );
}
