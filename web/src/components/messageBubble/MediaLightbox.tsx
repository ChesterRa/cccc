import { FloatingPortal } from "@floating-ui/react";
import { useId } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import { classNames } from "../../utils/classNames";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { ModalFocusGuard } from "../ModalFocusGuard";
import { CloseIcon, DownloadIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";
import "./mediaLightbox.css";

/** The full-screen dialog an attachment preview opens into. */
export function MediaLightbox({
  open,
  onClose,
  dialogLabel,
  title,
  downloadHref,
  downloadName,
  image = false,
  children,
}: {
  open: boolean;
  onClose: () => void;
  dialogLabel: string;
  title: string;
  downloadHref: string;
  downloadName: string;
  image?: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation("chat");
  const { modalRef } = useModalA11y(open, onClose);
  const hintId = useId();
  if (!open) return null;
  return (
    <FloatingPortal>
      <div className="fixed inset-0 z-[80] animate-fade-in">
        <div
          className={classNames(
            "relative flex h-[100dvh] w-full flex-col overflow-hidden",
            "bg-[var(--color-bg-primary)]",
          )}
          ref={modalRef}
          role="dialog"
          aria-modal="true"
          aria-label={dialogLabel}
          aria-describedby={hintId}
          onClick={(event) => event.stopPropagation()}
          // Attachments open from chat messages, which take the LCD look when it is on.
          data-lcd-scope
          data-media-lightbox
        >
          <div
            className={classNames(
              "flex items-center justify-between gap-3 border-b",
              "border-[var(--glass-border-subtle)]",
            )}
            data-media-header
          >
            <div className="min-w-0" data-media-title>
              <p
                className={classNames(
                  "truncate text-sm font-medium",
                  "text-[var(--color-text-primary)]",
                )}
              >
                {title}
              </p>
              <p
                className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}
                data-media-hint
                id={hintId}
              >
                {t(image ? "imagePreviewHint" : "videoPreviewHint")}
              </p>
            </div>

            <div className="flex shrink-0 items-center gap-2" data-media-actions>
              <AuthenticatedDownloadLink
                href={downloadHref}
                download={downloadName}
                className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11 max-sm:pointer-coarse:h-[44px] max-sm:pointer-coarse:w-[44px]`}
                title={t("download", { name: title })}
                aria-label={t("download", { name: title })}
                data-lcd-key
              >
                <DownloadIcon size={18} aria-hidden="true" />
              </AuthenticatedDownloadLink>

              <button
                type="button"
                onClick={onClose}
                className={`${buttonVariants({ variant: "ghost", size: "icon" })} max-sm:h-11 max-sm:w-11 max-sm:pointer-coarse:h-[44px] max-sm:pointer-coarse:w-[44px]`}
                aria-label={t("common:close")}
                data-lcd-key
              >
                <CloseIcon size={18} />
              </button>
            </div>
          </div>

          <div className="min-h-0 flex-1" data-media-content>
            {children}
          </div>
          <ModalFocusGuard />
        </div>
      </div>
    </FloatingPortal>
  );
}
