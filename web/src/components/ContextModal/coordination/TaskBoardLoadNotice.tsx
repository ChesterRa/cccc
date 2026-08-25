import { classNames } from "../../../utils/classNames";
import type { ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

export function TaskBoardLoadNotice({
  error,
  loading,
  tr,
  ui,
  onRetry,
}: {
  error: string;
  loading: boolean;
  tr: ContextTranslator;
  ui: ContextModalUi;
  onRetry: () => void;
}) {
  if (error) {
    return (
      <div
        className="flex items-center justify-between gap-3 rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-600 dark:text-rose-400"
        role="alert"
      >
        <span>{error || tr("context.failedToLoadTasks", "Failed to load tasks")}</span>
        <button type="button" className={ui.buttonSecondaryClass} onClick={onRetry}>
          {tr("common:retry", "Retry")}
        </button>
      </div>
    );
  }
  if (!loading) return null;
  return (
    <div className={classNames("text-sm", ui.mutedTextClass)} role="status">
      {tr("context.loadingTasks", "Loading tasks…")}
    </div>
  );
}
