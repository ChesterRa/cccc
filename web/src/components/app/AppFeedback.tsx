import { useTranslation } from "react-i18next";

type AppNotice = { message: string; actionLabel?: string; onAction?: () => void };

/**
 * Daemon errors quote commands in backticks. Render each word of one as an inline block, so a
 * word that fits on a line, such as `--runtime`, never breaks at its hyphens; a word wider than
 * the line, such as a 32-character ID, still wraps inside its block. The copied text stays the
 * original.
 */
export function FeedbackText({ text }: { text: string }) {
  const parts = text.split("`");
  if (parts.length < 3) return <>{text}</>;
  return (
    <>
      {parts.map((part, index) => {
        // An unpaired final backtick leaves its text as it was written.
        const quoted = index % 2 === 1 && index < parts.length - 1;
        if (!quoted) return index % 2 === 1 ? `\`${part}` : part;
        return (
          <code key={index}>
            {part.split(/(\s+)/).map((word, wordIndex) =>
              /^\s+$/.test(word) || !word ? (
                word
              ) : (
                <span key={wordIndex} className="inline-block max-w-full [overflow-wrap:anywhere]">
                  {word}
                </span>
              ),
            )}
          </code>
        );
      })}
    </>
  );
}

type AppFeedbackProps = {
  webReadOnly: boolean;
  errorMsg: string;
  notice: AppNotice | null;
  dismissError: () => void;
  dismissNotice: () => void;
};

export function AppFeedback({
  webReadOnly,
  errorMsg,
  notice,
  dismissError,
  dismissNotice,
}: AppFeedbackProps) {
  const { t } = useTranslation(["layout", "common"]);

  const handleNoticeAction = () => {
    if (!notice?.onAction) return;
    notice.onAction();
    dismissNotice();
  };

  if (webReadOnly || (!errorMsg && !notice)) {
    return null;
  }

  return (
    <div className="pointer-events-none fixed inset-x-0 top-4 z-[1200] flex flex-col items-center gap-3 px-4">
      {errorMsg ? (
        <div
          className="pointer-events-auto flex w-full max-w-xl items-center gap-3 px-4 py-3 text-sm text-[var(--destructive)] glass-modal"
          role="alert"
        >
          <span className="min-w-0 flex-1 break-words leading-6">
            <FeedbackText text={errorMsg} />
          </span>
          <button
            type="button"
            className="glass-btn flex h-10 w-10 shrink-0 items-center justify-center p-2"
            onClick={dismissError}
            aria-label={t("layout:dismissError")}
          >
            ×
          </button>
        </div>
      ) : null}

      {notice ? (
        <div
          className="pointer-events-auto flex w-full max-w-xl items-center gap-3 px-4 py-3 text-sm text-[var(--color-text-primary)] glass-modal"
          role="status"
        >
          <span className="min-w-0 flex-1 break-words">{notice.message}</span>
          {notice.onAction && notice.actionLabel ? (
            <button
              type="button"
              className="glass-btn px-2 py-1 text-xs"
              onClick={handleNoticeAction}
              data-lcd-key
            >
              {notice.actionLabel}
            </button>
          ) : null}
          <button
            type="button"
            className="glass-btn flex min-h-[36px] min-w-[36px] items-center justify-center p-2 text-[var(--color-text-secondary)]"
            onClick={dismissNotice}
            aria-label={t("common:dismiss")}
          >
            ×
          </button>
        </div>
      ) : null}
    </div>
  );
}
