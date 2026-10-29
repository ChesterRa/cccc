import { useLayoutEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useModalStore } from "../../stores/useModalStore";

/**
 * Enabling stays in settings, where the permission and subscription steps
 * each run from the user's own click.
 */
export function NotificationPrompt({
  onDismiss,
  restoreFocus,
}: {
  onDismiss: () => void;
  restoreFocus: () => void;
}) {
  const { t } = useTranslation("chat");
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const rootRef = useRef<HTMLDivElement>(null);
  const restoreFocusRef = useRef(restoreFocus);
  useLayoutEffect(() => {
    restoreFocusRef.current = restoreFocus;
  });
  useLayoutEffect(() => {
    const root = rootRef.current;
    return () => {
      // However the offer goes away (Not now, turning notifications on, or the
      // last request being answered elsewhere), focus inside it moves on
      // instead of falling back to the page. Focus elsewhere stays put.
      if (root?.contains(document.activeElement)) restoreFocusRef.current();
    };
  }, []);
  return (
    <div ref={rootRef} className="flex-shrink-0 px-2 pb-1.5 sm:px-2.5">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-3 px-3 text-xs text-[var(--color-text-secondary)]">
        <span>{t("notificationPrompt.text")}</span>
        <button
          type="button"
          className="min-h-9 underline"
          onClick={() => openSettingsTarget({ scope: "global", tab: "notifications" })}
        >
          {t("notificationPrompt.enable")}
        </button>
        <button type="button" className="min-h-9 underline" onClick={onDismiss}>
          {t("notificationPrompt.dismiss")}
        </button>
      </div>
    </div>
  );
}
