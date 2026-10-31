import { useTranslation } from "react-i18next";
import { useNoticeFocusRestore } from "./useNoticeFocusRestore";

export function WebUpdateNotice({
  onDismiss,
  onReload,
}: {
  onDismiss: () => void;
  onReload: () => void;
}) {
  const { t } = useTranslation("layout");
  const { rootRef, onFocusCapture } = useNoticeFocusRestore();

  return (
    <aside
      ref={rootRef}
      data-lcd-scope
      onFocusCapture={onFocusCapture}
      className="border-b border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-3 py-2 text-sm text-[var(--color-text-primary)]"
    >
      <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-3 gap-y-2">
        <p role="status" className="min-w-0 flex-1 basis-48">
          {t("webUpdateAvailable")}
        </p>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            data-lcd-key
            className="min-h-9 rounded-md border border-[var(--color-border-primary)] px-3 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
            onClick={() => {
              if (window.confirm(t("webUpdateReloadConfirm"))) onReload();
            }}
          >
            {t("webUpdateReload")}
          </button>
          <button
            type="button"
            data-lcd-key
            className="min-h-9 rounded-md border border-[var(--color-border-primary)] px-3 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
            onClick={onDismiss}
          >
            {t("webUpdateLater")}
          </button>
        </div>
      </div>
    </aside>
  );
}
