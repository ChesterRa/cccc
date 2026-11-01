import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StopIcon, TerminalIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { RUNTIME_INFO } from "../../types";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { VoiceAcpConsole } from "./VoiceAcpConsole";
import { VoiceAnalystTerminal } from "./VoiceAnalystTerminal";

/** One line for a collapsed Analyst: what it is, whether it is ready, and what needs the user. */
export function CodexVoiceAnalystSummaryBar({
  controller,
  analystPhase,
  notReady,
  attentionCount,
  errorText,
  hasNewResult,
  onExpand,
  actions,
}: {
  controller: CodexVoiceSessionController;
  analystPhase: string;
  notReady: boolean;
  attentionCount: number;
  errorText: string;
  hasNewResult: boolean;
  onExpand(): void;
  /** Controls for the Analyst's current work, before the expand action. */
  actions?: ReactNode;
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
      <span className="font-semibold text-[var(--color-text-primary)]">{t("chet.activity")}</span>
      {runtimeLabel ? (
        <span className="text-[var(--color-text-muted)]">· {runtimeLabel}</span>
      ) : null}
      {notReady ? (
        <span className="text-amber-700 dark:text-amber-300" data-status-tone="warning">
          · {t("codexVoiceAnalystNotReady")}
        </span>
      ) : analystPhase ? (
        <span className="min-w-0 truncate text-[var(--color-text-muted)]">· {analystPhase}</span>
      ) : null}
      {attentionCount ? (
        <span
          className="rounded-full bg-amber-400/15 px-2 py-0.5 font-semibold text-amber-700 dark:text-amber-200"
          data-pending-state
        >
          {t("codexVoiceAnalystNeedsInput", { count: attentionCount })}
        </span>
      ) : null}
      {errorText ? (
        <span
          className="min-w-0 max-w-full truncate text-rose-600 dark:text-rose-300"
          data-status-tone="error"
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
      <span className="ml-auto flex flex-wrap items-center gap-1">
        {actions}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={onExpand}
          aria-expanded={false}
          aria-controls="codex-voice-analyst-pane"
          data-lcd-key
        >
          {t("codexVoiceShowAnalyst")}
        </Button>
      </span>
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
  const cancelLabel = t(
    analyst?.structured ? "actors:acpControls.cancel" : "codexVoiceCancelInvestigation",
  );

  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}
      id="codex-voice-analyst-pane"
      aria-labelledby="codex-voice-analyst-heading"
    >
      <PaneHeader>
        {/* The key moves below the title only when the title itself could not sit beside it. */}
        <div className="min-w-0 flex-[1_1_10rem]">
          <div className="flex min-w-0 items-center gap-2">
            <TerminalIcon size={16} className="flex-none text-[var(--color-accent-primary)]" />
            <h3
              id="codex-voice-analyst-heading"
              className="flex-none whitespace-nowrap text-sm font-semibold text-[var(--color-text-primary)]"
            >
              {t("chet.activity")}
            </h3>
            {runtimeLabel ? (
              <span className="truncate text-xs text-[var(--color-text-muted)]">
                · {runtimeLabel}
              </span>
            ) : null}
          </div>
          {notReady ? (
            <p
              className="mt-1 text-xs text-amber-700 dark:text-amber-300"
              data-status-tone="warning"
            >
              {t("codexVoiceAnalystNotReady")}
            </p>
          ) : analystPhase ? (
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">{analystPhase}</p>
          ) : null}
        </div>
        <div className="flex min-w-0 items-center gap-1">
          {controller.analystWorking ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="max-sm:min-h-11 max-sm:min-w-11"
              aria-label={cancelLabel}
              data-lcd-key
              onClick={() => void controller.cancelInvestigation()}
            >
              <StopIcon size={14} />
              <span className="hidden sm:inline">{cancelLabel}</span>
            </Button>
          ) : analyst ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="max-sm:min-h-11"
              data-lcd-key
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
          data-status-tone="warning"
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
            showReadStatus={controller.owned}
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

function PaneHeader({ className = "flex", children }: { className?: string; children: ReactNode }) {
  return (
    <div
      className={`${className} min-h-14 flex-none items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] flex-wrap px-4 py-2 sm:px-5`}
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
