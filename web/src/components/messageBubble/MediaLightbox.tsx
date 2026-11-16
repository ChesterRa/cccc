import { FloatingPortal } from "@floating-ui/react";
import { ExternalLink } from "lucide-react";
import { useId } from "react";
import type { ReactNode, Ref } from "react";
import { useTranslation } from "react-i18next";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useChatObscuringView } from "../../hooks/useChatObscuringView";
import { classNames } from "../../utils/classNames";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { ModalFocusGuard } from "../ModalFocusGuard";
import { CloseIcon, DownloadIcon, LcdIcon } from "../Icons";
import { dialogIconKey } from "../ui/toolbarKey";
import "./mediaLightbox.css";

/** The full-screen dialog an attachment preview opens into. */
export function MediaLightbox({
  open,
  onClose,
  dialogLabel,
  title,
  downloadHref,
  downloadName,
  openHref,
  image = false,
  actionsRef,
  children,
}: {
  open: boolean;
  onClose: () => void;
  dialogLabel: string;
  title: string;
  /** Left out for an address on another site, where a download link would replace CCCC. */
  downloadHref?: string;
  downloadName?: string;
  /** Hands the file to the browser's own viewer, for what the page cannot show well. */
  openHref?: string;
  image?: boolean;
  /** A document puts its source switch beside the download action. */
  actionsRef?: Ref<HTMLSpanElement>;
  children: ReactNode;
}) {
  const { t } = useTranslation("chat");
  const { modalRef } = useModalA11y(open, onClose);
  useChatObscuringView(open);
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
              {openHref ? (
                <a
                  href={openHref}
                  target="_blank"
                  rel="noopener noreferrer"
                  className={dialogIconKey}
                  title={t("workspaceOpenSeparate")}
                  aria-label={t("workspaceOpenSeparate")}
                  data-lcd-key
                >
                  <LcdIcon
                    icon={ExternalLink}
                    glyph="external-link"
                    className="h-[18px] w-[18px]"
                  />
                </a>
              ) : null}
              <span ref={actionsRef} className="contents" />
              {downloadHref ? (
                <AuthenticatedDownloadLink
                  href={downloadHref}
                  download={downloadName}
                  className={dialogIconKey}
                  title={t("download", { name: title })}
                  aria-label={t("download", { name: title })}
                  data-lcd-key
                >
                  <DownloadIcon size={18} aria-hidden="true" />
                </AuthenticatedDownloadLink>
              ) : null}

              <button
                type="button"
                onClick={onClose}
                className={dialogIconKey}
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
