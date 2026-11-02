import { useTranslation } from "react-i18next";

type AppNotice = { message: string; actionLabel?: string; onAction?: () => void };

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
          <span className="min-w-0 flex-1 break-words leading-6">{errorMsg}</span>
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
