import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";

/**
 * Takes the place of the message filters while the chat shows one task's conversation: which
 * task, how much of it is shown, and the way back to the full conversation.
 */
export function TaskViewBanner({
  taskId,
  title,
  count,
  loading,
  error,
  truncated,
  contextIncomplete,
  onDetails,
  onRetry,
  onExit,
}: {
  taskId: string;
  title: string;
  count: number;
  loading: boolean;
  error: boolean;
  truncated: boolean;
  contextIncomplete: boolean;
  onDetails: () => void;
  onRetry: () => void;
  onExit: () => void;
}) {
  const { t } = useTranslation("chat");
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const target = event.target instanceof Element ? event.target : null;
      // Escape inside a field, a dialog or a menu belongs to that control.
      if (
        target?.closest(
          'input, textarea, select, [contenteditable="true"], [role="dialog"], [role="menu"]',
        )
      )
        return;
      onExit();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onExit]);

  const note = error
    ? t("taskView.error")
    : truncated
      ? t("taskView.truncated")
      : contextIncomplete
        ? t("taskView.contextIncomplete")
        : "";
  return (
    <div
      className="flex min-h-[44px] min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1 text-sm md:min-h-9"
      role="status"
      data-task-view={taskId}
    >
      <span className="flex-none text-[var(--color-text-secondary)]">{t("taskView.viewing")}</span>
      <span className="flex-none font-semibold text-[var(--color-text-primary)]">{taskId}</span>
      {title ? (
        <span
          className="min-w-0 max-w-[24rem] truncate text-[var(--color-text-primary)]"
          title={title}
        >
          {title}
        </span>
      ) : null}
      <span className="flex-none text-xs text-[var(--color-text-tertiary)]" data-task-view-count>
        {loading ? t("taskView.loading") : t("taskView.count", { count })}
      </span>
      {note ? (
        <span
          className="min-w-0 text-xs text-amber-700 dark:text-amber-300"
          data-task-view-note={error ? "error" : truncated ? "truncated" : "incomplete"}
        >
          {note}
        </span>
      ) : null}
      <span className="ml-auto flex flex-none items-center gap-1">
        {error ? (
          <Button type="button" variant="ghost" size="sm" data-lcd-key onClick={onRetry}>
            {t("taskView.retry")}
          </Button>
        ) : null}
        <Button type="button" variant="ghost" size="sm" data-lcd-key onClick={onDetails}>
          {t("taskView.details")}
        </Button>
        <Button type="button" variant="outline" size="sm" data-lcd-key onClick={onExit}>
          {t("taskView.exit")}
        </Button>
      </span>
    </div>
  );
}
