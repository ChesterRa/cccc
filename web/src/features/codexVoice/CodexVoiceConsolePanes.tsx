import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StopIcon, TerminalIcon, VoiceWaveformIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { RUNTIME_INFO } from "../../types";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceAcpConsole } from "./VoiceAcpConsole";
import { VoiceAnalystTerminal } from "./VoiceAnalystTerminal";

export function CodexVoiceConversationPane({
  controller,
  visible,
  children,
  analystExpanded,
  onToggleAnalyst,
  analystBar,
}: {
  controller: CodexVoiceSessionController;
  visible: boolean;
  children?: ReactNode;
  analystExpanded: boolean;
  onToggleAnalyst(): void;
  analystBar?: ReactNode;
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
      <PaneHeader className="hidden lg:flex">
        <h3
          id="codex-voice-conversation-heading"
          className="text-sm font-semibold text-[var(--color-text-primary)]"
        >
          {t("codexVoiceConversation")}
        </h3>
        {analystExpanded ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onToggleAnalyst}
            aria-expanded
            aria-controls="codex-voice-analyst-pane"
          >
            {t("codexVoiceHideAnalyst")}
          </Button>
        ) : null}
      </PaneHeader>
      {children}
      <div
        ref={conversationRef}
        className="min-h-0 flex-1 space-y-5 overflow-y-auto px-5 py-5"
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
          controller.isEngaged ? (
            <PaneEmptyState
              icon={<VoiceWaveformIcon size={22} />}
              title={t("codexVoiceConversationListening")}
              live
            />
          ) : (
            <PaneEmptyState
              icon={<VoiceWaveformIcon size={22} />}
              title={t("codexVoiceConversationEmptyTitle")}
              hint={t("codexVoiceConversationReady")}
            />
          )
        ) : null}
      </div>
      {controller.conversation.length ? (
        <p className="flex-none border-t border-[var(--glass-border-subtle)] px-5 py-2 text-[11px] text-[var(--color-text-muted)]">
          {t("codexVoiceConversationHistoryHint")}
        </p>
      ) : null}
      {analystBar}
    </section>
  );
}

/** One line for a collapsed Analyst: what it is, whether it is ready, and what needs the user. */
export function CodexVoiceAnalystSummaryBar({
  controller,
  analystPhase,
  notReady,
  attentionCount,
  errorText,
  hasNewResult,
  onExpand,
}: {
  controller: CodexVoiceSessionController;
  analystPhase: string;
  notReady: boolean;
  attentionCount: number;
  errorText: string;
  hasNewResult: boolean;
  onExpand(): void;
}) {
  const { t } = useTranslation("modals");
  const runtime = controller.readiness?.analyst_runtime;
  const runtimeLabel = runtime ? RUNTIME_INFO[runtime]?.label || runtime : "";
  return (
    <div
      data-codex-voice-analyst-bar
      className="hidden min-h-[52px] flex-none flex-wrap items-center gap-x-2 gap-y-1 border-t border-[var(--glass-border-subtle)] px-5 py-2 text-xs lg:flex"
    >
      <TerminalIcon
        size={15}
        aria-hidden="true"
        className="flex-none text-[var(--color-accent-primary)]"
      />
      <span className="font-semibold text-[var(--color-text-primary)]">
        {t("codexVoiceAnalystTitle")}
      </span>
      {runtimeLabel ? (
        <span className="text-[var(--color-text-muted)]">· {runtimeLabel}</span>
      ) : null}
      {notReady ? (
        <span className="text-amber-700 dark:text-amber-300">
          · {t("codexVoiceAnalystNotReady")}
        </span>
      ) : analystPhase ? (
        <span className="min-w-0 truncate text-[var(--color-text-muted)]">· {analystPhase}</span>
      ) : null}
      {attentionCount ? (
        <span className="rounded-full bg-amber-400/15 px-2 py-0.5 font-semibold text-amber-700 dark:text-amber-200">
          {t("codexVoiceAnalystNeedsInput", { count: attentionCount })}
        </span>
      ) : null}
      {errorText ? (
        <span
          className="min-w-0 max-w-full truncate text-rose-600 dark:text-rose-300"
          title={errorText}
        >
          {errorText}
        </span>
      ) : null}
      {hasNewResult ? (
        <span className="rounded-full bg-[var(--glass-tab-bg-active)] px-2 py-0.5 font-semibold text-[var(--color-accent-primary)]">
          {t("codexVoiceAnalystNewResult")}
        </span>
      ) : null}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="ml-auto"
        onClick={onExpand}
        aria-expanded={false}
        aria-controls="codex-voice-analyst-pane"
      >
        {t("codexVoiceShowAnalyst")}
      </Button>
    </div>
  );
}

export function CodexVoiceAnalystPane({
  controller,
  analystPhase,
  visible,
  terminalVisible,
  isDark,
  notReady = false,
}: {
  controller: CodexVoiceSessionController;
  analystPhase: string;
  visible: boolean;
  terminalVisible: boolean;
  isDark?: boolean;
  notReady?: boolean;
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
      <PaneHeader>
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
          {notReady ? (
            <p className="mt-1 text-xs text-amber-700 dark:text-amber-300">
              {t("codexVoiceAnalystNotReady")}
            </p>
          ) : analystPhase ? (
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
              {t(
                analyst?.structured ? "actors:acpControls.cancel" : "codexVoiceCancelInvestigation",
              )}
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
      </PaneHeader>

      {controller.analystWarning &&
      !(analyst?.structured && analyst.warning === "analyst_turn_failed" && analyst.last_error) ? (
        <div
          className="flex-none border-b border-amber-400/25 bg-amber-400/8 px-5 py-2.5 text-xs leading-5 text-amber-700 dark:text-amber-300"
          role="status"
        >
          {controller.analystWarning}
        </div>
      ) : null}

      <div className="min-h-0 flex-1">
        {analyst?.structured ? (
          <VoiceAcpConsole
            key={analyst.generation}
            analyst={analyst}
            visible={terminalVisible}
            call={controller.owned ? controller.call : null}
            isDark={isDark}
            onAnalystSnapshot={controller.updateAnalystSnapshot}
          />
        ) : analyst?.tui_ready ? (
          <VoiceAnalystTerminal analyst={analyst} isVisible={terminalVisible} runtime={runtime} />
        ) : (
          <PaneEmptyState
            icon={<TerminalIcon size={22} />}
            title={t("codexVoiceAnalystEmptyTitle")}
            hint={t("codexVoiceAnalystTerminalPending")}
          />
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

/** Both panes share one header height so their dividers line up. */
function PaneHeader({ className = "flex", children }: { className?: string; children: ReactNode }) {
  return (
    <div
      className={`${className} min-h-14 flex-none items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-2 sm:px-5`}
    >
      {children}
    </div>
  );
}

function PaneEmptyState({
  icon,
  title,
  hint,
  live = false,
}: {
  icon: ReactNode;
  title: string;
  hint?: string;
  live?: boolean;
}) {
  return (
    <div className="flex h-full min-h-56 flex-col items-center justify-center px-8 py-10 text-center">
      <div
        className={`flex h-12 w-12 items-center justify-center rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] ${live ? "animate-pulse" : ""}`}
        aria-hidden="true"
      >
        {icon}
      </div>
      <p className="mt-4 text-sm font-medium text-[var(--color-text-primary)]">{title}</p>
      {hint ? (
        <p className="mt-1.5 max-w-sm text-xs leading-5 text-[var(--color-text-muted)]">{hint}</p>
      ) : null}
    </div>
  );
}
