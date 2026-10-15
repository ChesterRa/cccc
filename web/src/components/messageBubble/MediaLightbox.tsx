import { FloatingPortal } from "@floating-ui/react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import { classNames } from "../../utils/classNames";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { ModalFocusGuard } from "../ModalFocusGuard";
import { CloseIcon, DownloadIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";

/** The full-screen dialog an attachment preview opens into. */
export function MediaLightbox({
  open,
  onClose,
  dialogLabel,
  title,
  downloadHref,
  downloadName,
  children,
}: {
  open: boolean;
  onClose: () => void;
  dialogLabel: string;
  title: string;
  downloadHref: string;
  downloadName: string;
  children: ReactNode;
}) {
  const { t } = useTranslation("chat");
  const { modalRef } = useModalA11y(open, onClose);
  if (!open) return null;
  return (
    <FloatingPortal>
      <div className="fixed inset-0 z-[80] flex items-center justify-center sm:p-4 animate-fade-in">
        <button
          type="button"
          className={classNames("absolute inset-0", "glass-overlay")}
          onClick={onClose}
          aria-label={t("common:close")}
        />

        <div
          className={classNames(
            "relative z-[81] flex h-[100dvh] sm:h-[90dvh] w-full max-w-5xl flex-col overflow-hidden rounded-2xl border shadow-2xl",
            "glass-modal",
          )}
          ref={modalRef}
          role="dialog"
          aria-modal="true"
          aria-label={dialogLabel}
          onClick={(event) => event.stopPropagation()}
        >
          <div
            className={classNames(
              "flex items-center justify-between gap-3 border-b px-4 py-3",
              "border-[var(--glass-border-subtle)]",
            )}
          >
            <div className="min-w-0">
              <p
                className={classNames(
                  "truncate text-sm font-medium",
                  "text-[var(--color-text-primary)]",
                )}
              >
                {title}
              </p>
              <p className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}>
                {t("imagePreviewHint")}
              </p>
            </div>

            <div className="flex shrink-0 items-center gap-2">
              <AuthenticatedDownloadLink
                href={downloadHref}
                download={downloadName}
                className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11`}
                title={t("download", { name: title })}
                aria-label={t("download", { name: title })}
              >
                <DownloadIcon size={18} aria-hidden="true" />
              </AuthenticatedDownloadLink>

              <button
                type="button"
                onClick={onClose}
                className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11`}
                aria-label={t("common:close")}
              >
                <CloseIcon size={18} />
              </button>
            </div>
          </div>

          <div className="min-h-0 flex-1">{children}</div>
          <ModalFocusGuard />
        </div>
      </div>
    </FloatingPortal>
  );
}
