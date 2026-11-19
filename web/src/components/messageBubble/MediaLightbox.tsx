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
import type { AttachmentResourceMedia } from "../resourcePanel/views/AttachmentResourceView";
import { dialogIconKey } from "../ui/toolbarKey";
import { AttachmentStepper } from "./AttachmentStepper";
import { attachmentStepKey } from "./attachmentStepKey";
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
  step,
  leadingActions,
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
  /** Previous and next through the other attachments of the same message. */
  step?: {
    position: number;
    count: number;
    media: AttachmentResourceMedia;
    onStep: (delta: -1 | 1) => void;
  };
  /** Keys before the file's own actions. */
  leadingActions?: ReactNode;
  children: ReactNode;
}) {
  const { t } = useTranslation("chat");
  // Opened from a message: closing must not scroll the conversation to the attachment.
  const { modalRef } = useModalA11y(open, onClose, { preventReturnScroll: true });
  const stepping = step && step.count > 1 ? step : null;
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
          onKeyDown={
            stepping
              ? (event) => {
                  const delta = attachmentStepKey(event, stepping.media, "[data-media-content]");
                  if (!delta) return;
                  event.preventDefault();
                  stepping.onStep(delta);
                }
              : undefined
          }
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
            <div className="flex min-w-0 flex-1 items-center gap-2" data-media-title>
              {/* Before the name, so the keys keep their place while the actions on the right
                  change with each file. */}
              {stepping ? (
                <AttachmentStepper
                  position={stepping.position}
                  count={stepping.count}
                  onStep={stepping.onStep}
                  keyClassName={dialogIconKey}
                  iconSize={18}
                />
              ) : null}
              <div className="min-w-0">
                <p
                  className={classNames(
                    "truncate text-sm font-medium",
                    "text-[var(--color-text-primary)]",
                  )}
                >
                  {title}
                </p>
                <p
                  className={classNames(
                    "text-xs",
                    "text-[var(--color-text-tertiary)]",
                    // On a phone the previous and next keys take the room the hint had; the
                    // file name keeps its line, and the hint stays the dialog's description.
                    stepping && "max-sm:sr-only",
                  )}
                  data-media-hint
                  id={hintId}
                >
                  {t(image ? "imagePreviewHint" : "videoPreviewHint")}
                </p>
              </div>
            </div>

            <div className="flex shrink-0 items-center gap-2" data-media-actions>
              {leadingActions}
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
