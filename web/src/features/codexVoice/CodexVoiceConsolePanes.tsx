import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { StopIcon, TerminalIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceAnalystTerminal } from "./VoiceAnalystTerminal";

export function CodexVoiceConversationPane({
  controller,
  phaseLabel,
  visible,
}: {
  controller: CodexVoiceSessionController;
  phaseLabel: string;
  visible: boolean;
}) {
  const { t } = useTranslation("modals");
  const conversationRef = useRef<HTMLDivElement | null>(null);
  const followTranscriptRef = useRef(true);

  useEffect(() => {
    if (!visible || !followTranscriptRef.current) return;
    const frame = window.requestAnimationFrame(() => {
      const container = conversationRef.current;
      if (container) container.scrollTop = container.scrollHeight;
    });
    return () => window.cancelAnimationFrame(frame);
  }, [controller.assistantTranscript, controller.userTranscript, visible]);

  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 flex-col border-r border-[var(--glass-border-subtle)] lg:flex`}
      aria-labelledby="codex-voice-conversation-heading"
    >
      <div className="hidden flex-none items-center justify-between border-b border-[var(--glass-border-subtle)] px-5 py-3 lg:flex">
        <h3
          id="codex-voice-conversation-heading"
          className="text-sm font-semibold text-[var(--color-text-primary)]"
        >
          {t("codexVoiceConversation")}
        </h3>
        <span className="text-xs text-[var(--color-text-muted)]">{phaseLabel}</span>
      </div>
      <div
        ref={conversationRef}
        className="min-h-0 flex-1 space-y-5 overflow-y-auto px-5 py-5"
        aria-live="polite"
        aria-atomic="false"
        onScroll={(event) => {
          const element = event.currentTarget;
          followTranscriptRef.current =
            element.scrollHeight - element.clientHeight - element.scrollTop < 80;
        }}
      >
        {controller.userTranscript ? (
          <TranscriptBlock label={t("codexVoiceYouSaid")} text={controller.userTranscript} />
        ) : null}
        {controller.assistantTranscript ? (
          <TranscriptBlock
            label={t("codexVoiceAssistantSaid")}
            text={controller.assistantTranscript}
          />
        ) : null}
        {!controller.userTranscript && !controller.assistantTranscript ? (
          <div className="flex min-h-40 items-center justify-center text-center text-sm leading-6 text-[var(--color-text-muted)]">
            {controller.isEngaged
              ? t("codexVoiceConversationListening")
              : t("codexVoiceConversationReady")}
          </div>
        ) : null}
      </div>
    </section>
  );
}

export function CodexVoiceAnalystPane({
  controller,
  analystPhase,
  visible,
  terminalVisible,
}: {
  controller: CodexVoiceSessionController;
  analystPhase: string;
  visible: boolean;
  terminalVisible: boolean;
}) {
  const { t } = useTranslation("modals");
  const analyst = controller.analyst;

  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 flex-col lg:flex`}
      aria-labelledby="codex-voice-analyst-heading"
    >
      <div className="flex flex-none items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5">
        <div className="flex min-w-0 items-center gap-2">
          <TerminalIcon size={16} className="text-[var(--color-accent-primary)]" />
          <h3
            id="codex-voice-analyst-heading"
            className="truncate text-sm font-semibold text-[var(--color-text-primary)]"
          >
            {t("codexVoiceAnalystTitle")}
          </h3>
          {analystPhase ? (
            <span className="truncate text-xs text-[var(--color-text-muted)]">
              · {analystPhase}
            </span>
          ) : null}
        </div>
        <div className="flex flex-none items-center gap-1">
          {controller.analystWorking ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => void controller.cancelInvestigation()}
            >
              <StopIcon size={14} />
              {t("codexVoiceCancelInvestigation")}
            </Button>
          ) : analyst ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={controller.isEngaged}
              onClick={() => {
                if (window.confirm(t("codexVoiceNewAnalystConfirm"))) {
                  void controller.startNewAnalyst();
                }
              }}
            >
              {t("codexVoiceNewAnalyst")}
            </Button>
          ) : null}
        </div>
      </div>

      {controller.analystWarning ? (
        <div
          className="flex-none border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-xs leading-5 text-amber-700 dark:text-amber-300"
          role="status"
        >
          {controller.analystWarning}
        </div>
      ) : null}

      <div className="min-h-0 flex-1">
        {analyst?.tui_ready ? (
          <VoiceAnalystTerminal analyst={analyst} isVisible={terminalVisible} />
        ) : (
          <div className="flex h-full min-h-56 flex-col items-center justify-center px-8 text-center">
            <TerminalIcon size={30} className="text-[var(--color-text-tertiary)]" />
            <p className="mt-3 max-w-md text-sm leading-6 text-[var(--color-text-muted)]">
              {t("codexVoiceAnalystTerminalPending")}
            </p>
          </div>
        )}
      </div>
    </section>
  );
}

function TranscriptBlock({ label, text }: { label: string; text: string }) {
  return (
    <div>
      <div className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--color-text-tertiary)]">
        {label}
      </div>
      <div className="mt-1.5 whitespace-pre-wrap text-[15px] leading-7 text-[var(--color-text-primary)]">
        {text}
      </div>
    </div>
  );
}
