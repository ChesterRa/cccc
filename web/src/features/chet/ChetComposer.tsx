import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import { SendIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/textarea";
import type { ProcessRecoveryOwner } from "../../services/api/processRecovery";
import { chetRefusalText } from "./chetText";
import type { ChetComposerProblem } from "./useChetConversation";

function Notice({
  tone,
  children,
  actions,
}: {
  tone: "warning" | "error" | "info";
  children: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div
      role={tone === "info" ? "status" : "alert"}
      data-status-tone={tone === "info" ? undefined : tone}
      className={`flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs leading-5 ${
        tone === "error"
          ? "text-rose-600 dark:text-rose-300"
          : tone === "warning"
            ? "text-amber-700 dark:text-amber-300"
            : "text-[var(--color-text-muted)]"
      }`}
    >
      <span className="min-w-0 flex-1 break-words">{children}</span>
      {actions ? (
        <span className="flex flex-none flex-wrap items-center gap-1">{actions}</span>
      ) : null}
    </div>
  );
}

/** Writes one message to Chet. The open group goes with it when it is sent. */
export function ChetComposer({
  draft,
  storageAvailable,
  problem,
  duplicateRisk,
  onText,
  onSend,
  onDismissProblem,
  onReviewRecovery,
  voiceStatus,
  voiceActions,
}: {
  draft: string;
  storageAvailable: boolean;
  problem: ChetComposerProblem | null;
  duplicateRisk: boolean;
  onText(text: string): void;
  onSend(): void;
  onDismissProblem(): void;
  onReviewRecovery(owners: ProcessRecoveryOwner[]): void;
  /** Voice choices and call state, shown at the start of the action row. */
  voiceStatus?: ReactNode;
  /** Voice controls placed beside Send. */
  voiceActions?: ReactNode;
}) {
  const { t } = useTranslation("modals");

  let notice: ReactNode = null;
  if (problem) {
    notice = (
      <Notice
        tone="error"
        actions={
          <>
            {problem.recoveryOwners.length ? (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                data-lcd-key
                onClick={() => onReviewRecovery(problem.recoveryOwners)}
              >
                {t("codexVoiceReviewRecoveryRecords")}
              </Button>
            ) : null}
            <Button type="button" variant="ghost" size="sm" data-lcd-key onClick={onDismissProblem}>
              {t("codexVoiceDismissError")}
            </Button>
          </>
        }
      >
        {t("chet.composer.refused", { error: chetRefusalText(t, problem.code, problem.message) })}
      </Notice>
    );
  } else if (duplicateRisk) {
    notice = <Notice tone="warning">{t("chet.composer.duplicateRisk")}</Notice>;
  } else if (!storageAvailable) {
    notice = <Notice tone="info">{t("chet.composer.storageUnavailable")}</Notice>;
  }

  return (
    <form
      className="flex-none border-t border-[var(--glass-border-subtle)] px-4 py-3 sm:px-6"
      onSubmit={(event) => {
        event.preventDefault();
        onSend();
      }}
    >
      <div className="mx-auto flex max-w-3xl flex-col gap-2">
        {notice}
        <Textarea
          className="min-h-16 max-h-40 resize-none"
          value={draft}
          rows={2}
          aria-label={t("chet.composer.inputLabel")}
          placeholder={t("chet.composer.placeholder")}
          title={t("chet.composer.sendHint")}
          onChange={(event) => onText(event.target.value)}
          onKeyDown={(event) => {
            const native = event.nativeEvent as KeyboardEvent & { keyCode?: number };
            if (native.isComposing || native.keyCode === 229) return;
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              onSend();
            }
          }}
        />
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">{voiceStatus}</div>
          <div className="ml-auto flex flex-none flex-wrap items-center justify-end gap-2">
            {voiceActions}
            <Button type="submit" size="sm" className="max-sm:min-h-11" disabled={!draft.trim()}>
              <SendIcon size={15} aria-hidden="true" />
              {t("chet.composer.send")}
            </Button>
          </div>
        </div>
      </div>
    </form>
  );
}
