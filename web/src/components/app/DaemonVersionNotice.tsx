import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { DaemonVersionDifference } from "../../utils/daemonVersion";

/** Identifies different versions without treating them as proof of incompatibility. */
export function DaemonVersionNotice({
  difference,
  onDismiss,
}: {
  difference: DaemonVersionDifference;
  onDismiss: () => void;
}) {
  const { t } = useTranslation("layout");
  const rootRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    const root = rootRef.current;
    return () => {
      if (!root?.contains(document.activeElement)) return;
      const previous = previousFocusRef.current;
      if (
        previous?.isConnected &&
        previous.getClientRects().length &&
        !previous.closest("[inert]")
      ) {
        previous.focus();
        if (document.activeElement === previous) return;
      }
      // If the notice was the first focus target, return to a visible app
      // header control rather than leaving keyboard focus on the page body.
      const fallback = Array.from(
        document.querySelectorAll<HTMLButtonElement>("main [data-group-header-work] button"),
      ).find((button) => {
        const rect = button.getBoundingClientRect();
        return (
          !button.disabled &&
          !button.closest('[inert], [aria-hidden="true"]') &&
          rect.width > 0 &&
          rect.height > 0 &&
          rect.right > 0 &&
          rect.bottom > 0 &&
          rect.left < window.innerWidth &&
          rect.top < window.innerHeight
        );
      });
      fallback?.focus();
    };
  }, []);

  return (
    <aside
      ref={rootRef}
      onFocusCapture={(event) => {
        const previous = event.relatedTarget;
        if (previous instanceof HTMLElement && !event.currentTarget.contains(previous))
          previousFocusRef.current = previous;
      }}
      className="relative z-10 shrink-0 overflow-y-auto overscroll-contain border-b border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-3 py-2 text-sm text-[var(--color-text-primary)]"
      style={{
        maxHeight: "max(4.5rem, min(14rem, calc(var(--app-viewport-height, 100dvh) - 200px)))",
      }}
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
