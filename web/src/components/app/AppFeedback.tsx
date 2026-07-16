import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";

type AppNotice = { message: string; actionLabel?: string; onAction?: () => void };

type AppFeedbackProps = {
  isDark: boolean;
  webReadOnly: boolean;
  errorMsg: string;
  notice: AppNotice | null;
  dismissError: () => void;
  dismissNotice: () => void;
};

export function AppFeedback({
  isDark,
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
          className={classNames(
            "pointer-events-auto flex w-full max-w-xl items-center gap-3 rounded-2xl px-4 py-3 text-sm shadow-2xl glass-modal animate-slide-up",
            isDark ? "border-rose-500/20 text-rose-300" : "border-rose-200/50 text-rose-700",
          )}
          role="alert"
        >
          <span className="min-w-0 flex-1 break-words leading-6">{errorMsg}</span>
          <button
            type="button"
            className={classNames(
              "glass-btn flex h-10 w-10 shrink-0 items-center justify-center rounded-lg p-2 transition-all",
              isDark ? "text-rose-400" : "text-rose-600",
            )}
            onClick={dismissError}
            aria-label={t("layout:dismissError")}
          >
            ×
          </button>
        </div>
      ) : null}

      {notice ? (
        <div
          className={classNames(
            "pointer-events-auto flex w-full max-w-xl items-center gap-3 rounded-2xl px-4 py-3 text-sm shadow-2xl glass-modal animate-slide-up",
            isDark ? "border-white/10 text-slate-200" : "border-black/10 text-gray-800",
          )}
          role="status"
        >
          <span className="min-w-0 flex-1 break-words">{notice.message}</span>
          {notice.onAction && notice.actionLabel ? (
            <button
              type="button"
              className={classNames(
                "glass-btn rounded-xl px-2 py-1 text-xs transition-all",
                isDark ? "text-slate-100" : "text-gray-900",
              )}
              onClick={handleNoticeAction}
            >
              {notice.actionLabel}
            </button>
          ) : null}
          <button
            type="button"
            className={classNames(
              "glass-btn flex min-h-[36px] min-w-[36px] items-center justify-center rounded-lg p-2 transition-all",
              isDark ? "text-slate-300" : "text-gray-600",
            )}
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
