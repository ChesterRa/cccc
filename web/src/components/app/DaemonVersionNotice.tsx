import { useTranslation } from "react-i18next";
import type { DaemonVersionDifference } from "../../utils/daemonVersion";
import { useNoticeFocusRestore } from "./useNoticeFocusRestore";

/** Identifies different versions without treating them as proof of incompatibility. */
export function DaemonVersionNotice({
  difference,
  onDismiss,
}: {
  difference: DaemonVersionDifference;
  onDismiss: () => void;
}) {
  const { t } = useTranslation("layout");
  const { rootRef, onFocusCapture } = useNoticeFocusRestore();

  return (
    <aside
      ref={rootRef}
      onFocusCapture={onFocusCapture}
      className="relative z-10 shrink-0 border-b border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-3 py-2 text-sm text-[var(--color-text-primary)]"
    >
      <div className="mx-auto flex max-w-5xl items-start gap-3">
        <div className="min-w-0 flex-1">
          <p role="status">{t("daemonVersionMismatch")}</p>
          <details className="mt-1 text-[var(--color-text-secondary)]">
            <summary className="cursor-pointer underline">{t("daemonVersionMismatchHelp")}</summary>
            <p className="mt-1 break-words">{t("daemonVersionMismatchDetail", difference)}</p>
          </details>
        </div>
        <button
          type="button"
          data-lcd-key
          className="flex min-h-9 min-w-9 shrink-0 items-center justify-center rounded-lg text-lg text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          onClick={onDismiss}
          aria-label={t("daemonVersionMismatchDismiss")}
        >
          ×
        </button>
      </div>
    </aside>
  );
}
