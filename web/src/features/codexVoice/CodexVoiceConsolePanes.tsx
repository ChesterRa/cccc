import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StopIcon, TerminalIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { RUNTIME_INFO } from "../../types";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceAnalystTerminal } from "./VoiceAnalystTerminal";

export function CodexVoiceConversationPane({
  controller,
  visible,
  children,
  analystExpanded,
  onToggleAnalyst,
}: {
  controller: CodexVoiceSessionController;
  visible: boolean;
  children?: ReactNode;
  analystExpanded: boolean;
  onToggleAnalyst(): void;
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
  }, [controller.conversation, visible]);

  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}
      id="codex-voice-conversation-pane"
      aria-labelledby="codex-voice-conversation-heading"
    >
      <div className="hidden flex-none items-center justify-between border-b border-[var(--glass-border-subtle)] px-5 py-3 lg:flex">
        <h3
          id="codex-voice-conversation-heading"
          className="text-sm font-semibold text-[var(--color-text-primary)]"
        >
          {t("codexVoiceConversation")}
        </h3>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={onToggleAnalyst}
          aria-expanded={analystExpanded}
        >
          <TerminalIcon size={15} />
          {t(analystExpanded ? "codexVoiceHideAnalyst" : "codexVoiceShowAnalyst")}
        </Button>
      </div>
      {children}
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
        {controller.conversation.map((turn) => (
          <TranscriptBlock
            key={turn.id}
            label={t(turn.role === "user" ? "codexVoiceYouSaid" : "codexVoiceAssistantSaid")}
            text={turn.text}
          />
        ))}
        {!controller.conversation.length ? (
          <div className="flex min-h-40 items-center justify-center text-center text-sm leading-6 text-[var(--color-text-muted)]">
            {controller.isEngaged
              ? t("codexVoiceConversationListening")
              : t("codexVoiceConversationReady")}
          </div>
        ) : null}
      </div>
      {controller.conversation.length ? (
        <p className="flex-none border-t border-[var(--glass-border-subtle)] px-5 py-2 text-[11px] text-[var(--color-text-muted)]">
          {t("codexVoiceConversationHistoryHint")}
        </p>
      ) : null}
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
  const runtime = controller.readiness?.analyst_runtime;
  const runtimeLabel = runtime ? RUNTIME_INFO[runtime]?.label || runtime : "";

  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}
      id="codex-voice-analyst-pane"
      aria-labelledby="codex-voice-analyst-heading"
    >
      <div className="flex flex-none items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <TerminalIcon size={16} className="flex-none text-[var(--color-accent-primary)]" />
            <h3
              id="codex-voice-analyst-heading"
              className="flex-none whitespace-nowrap text-sm font-semibold text-[var(--color-text-primary)]"
            >
              {t("codexVoiceAnalystTitle")}
            </h3>
            {runtimeLabel ? (
              <span className="truncate text-xs text-[var(--color-text-muted)]">
                · {runtimeLabel}
              </span>
            ) : null}
          </div>
          {analystPhase ? (
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">{analystPhase}</p>
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
          <VoiceAnalystTerminal analyst={analyst} isVisible={terminalVisible} runtime={runtime} />
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
      <div className="mt-1.5 whitespace-pre-wrap break-words text-[15px] leading-7 text-[var(--color-text-primary)]">
        {text}
      </div>
    </div>
  );
}
