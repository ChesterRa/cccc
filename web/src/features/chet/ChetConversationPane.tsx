import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StopIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import type { ProcessRecoveryOwner } from "../../services/api/processRecovery";
import type { CodexVoiceSessionController } from "../codexVoice/useCodexVoiceSessionController";
import { ChetComposer } from "./ChetComposer";
import { ChetTimeline } from "./ChetTimeline";
import { unsavedLiveTurns } from "./chetTimeline";
import type { ChetConversation } from "./useChetConversation";

/** Stops the shared Analyst's current work and its queue, whichever request started it. */
export function ChetStopWorkButton({
  controller,
  className,
}: {
  controller: CodexVoiceSessionController;
  className?: string;
}) {
  const { t } = useTranslation("actors");
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      data-lcd-key
      className={className}
      onClick={() => void controller.cancelInvestigation()}
    >
      <StopIcon size={14} aria-hidden="true" />
      {t("acpControls.cancel")}
    </Button>
  );
}

/**
 * Chet's conversation: one timeline above the composer. While the Analyst works, phones get
 * a stop line here; desktops find the same control on the Analyst bar.
 */
export function ChetConversationPane({
  controller,
  chet,
  isDark,
  visible,
  analystExpanded,
  onToggleAnalyst,
  analystBar,
  sources,
  voiceStatus,
  voiceActions,
  onReviewRecovery,
  onOpenGroupMessage,
}: {
  controller: CodexVoiceSessionController;
  chet: ChetConversation;
  isDark: boolean;
  visible: boolean;
  analystExpanded: boolean;
  onToggleAnalyst(): void;
  analystBar?: ReactNode;
  sources?: ReactNode;
  voiceStatus?: ReactNode;
  voiceActions?: ReactNode;
  onReviewRecovery(owners: ProcessRecoveryOwner[]): void;
  /** Opens a message in its Group; without it, Group messages offer no way there. */
  onOpenGroupMessage?: (groupId: string, eventId: string) => void;
}) {
  const { t } = useTranslation("modals");
  const hasUnknownSend = chet.items.some(
    (item) => item.kind === "request" && item.state === "send_unknown",
  );
  const notices = [
    chet.status?.history_write_failed ? t("chet.writeFailed") : "",
    chet.historyError ? t("chet.historyUnavailable", { error: chet.historyError }) : "",
  ].filter(Boolean);
  return (
    <section
      className={`${visible ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}
      id="codex-voice-conversation-pane"
      aria-label={t("chet.conversation")}
    >
      {analystExpanded ? (
        <div className="hidden min-h-14 flex-none items-center justify-end gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-2 sm:px-5 lg:flex">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onToggleAnalyst}
            aria-expanded
            aria-controls="codex-voice-analyst-pane"
            data-lcd-key
          >
            {t("codexVoiceHideAnalyst")}
          </Button>
        </div>
      ) : null}
      {sources}
      {/* Above the scrolling timeline, so a reader at its end still sees them. */}
      {notices.length ? (
        <div
          role="status"
          data-status-tone="warning"
          className="flex flex-none flex-col gap-1 border-b border-amber-400/25 bg-amber-400/8 px-4 py-2 text-xs text-amber-700 sm:px-6 dark:text-amber-300"
        >
          {notices.map((notice) => (
            <p key={notice}>{notice}</p>
          ))}
        </div>
      ) : null}
      <ChetTimeline
        items={chet.items}
        liveTurns={unsavedLiveTurns(chet.items, controller.conversation)}
        liveCallId={controller.call?.generation ?? null}
        loaded={chet.loaded}
        hasOlder={chet.hasOlder}
        loadingOlder={chet.loadingOlder}
        onLoadOlder={chet.loadOlder}
        isDark={isDark}
        onOpenGroupMessage={onOpenGroupMessage}
        notices={
          chet.skipped ? (
            <p className="text-xs text-[var(--color-text-muted)]">
              {t("chet.skipped", { count: chet.skipped })}
            </p>
          ) : null
        }
        actions={{
          problems: chet.problems,
          storageAvailable: chet.storageAvailable,
          sendAgainDisabled: hasUnknownSend,
          resentIds: chet.resentIds,
          onSendAgain: chet.sendAgain,
          onRetry: chet.retry,
          onPutBack: chet.putBack,
          onDismiss: chet.dismiss,
          onReviewRecovery,
        }}
      />
      {controller.analystWorking ? (
        <div
          className="flex flex-none flex-wrap items-center gap-2 border-t border-[var(--glass-border-subtle)] px-4 py-1.5 text-xs text-[var(--color-text-muted)] lg:hidden"
          data-chet-working
        >
          <span
            aria-hidden="true"
            data-status-dot="active"
            data-status-motion="working"
            className="h-2 w-2 flex-none rounded-full bg-emerald-400"
          />
          <span>{t("chet.assistantWorking")}</span>
          <ChetStopWorkButton controller={controller} className="ml-auto !min-h-8" />
        </div>
      ) : null}
      {analystBar}
      <ChetComposer
        draft={chet.draft}
        storageAvailable={chet.storageAvailable}
        problem={chet.composerProblem}
        duplicateRisk={chet.duplicateRisk}
        onText={chet.setText}
        onSend={chet.send}
        onDismissProblem={chet.clearComposerProblem}
        onReviewRecovery={onReviewRecovery}
        voiceStatus={voiceStatus}
        voiceActions={voiceActions}
      />
    </section>
  );
}
