import { useLayoutEffect, useRef } from "react";
import { Trans, useTranslation } from "react-i18next";
import { BellIcon } from "../../components/Icons";
import { buttonVariants } from "../../components/ui/button-variants";
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
      {/* The buttons move below the text when both do not fit on one line. */}
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-end gap-x-3 gap-y-1.5 rounded-lg border border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-3 py-2 text-sm leading-5 text-[var(--color-text-primary)] shadow-sm">
        <p className="flex min-w-0 flex-[1_1_16rem] items-start gap-2.5">
          <BellIcon size={18} className="mt-px shrink-0" aria-hidden="true" />
          <span className="min-w-0">
            <Trans
              i18nKey="notificationPrompt.text"
              ns="chat"
              components={[<strong key="lead" className="font-semibold" />]}
            />
          </span>
        </p>
        <div className="flex shrink-0 items-center gap-1.5">
          <button
            type="button"
            className={buttonVariants({ size: "sm" })}
            onClick={() => openSettingsTarget({ scope: "global", tab: "notifications" })}
          >
            {t("notificationPrompt.enable")}
          </button>
          <button
            type="button"
            className={buttonVariants({ variant: "ghost", size: "sm" })}
            onClick={onDismiss}
          >
            {t("notificationPrompt.dismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}
