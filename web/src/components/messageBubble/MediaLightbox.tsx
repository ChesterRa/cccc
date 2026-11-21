import { FloatingPortal } from "@floating-ui/react";
import { ExternalLink } from "lucide-react";
import { useEffect, useId } from "react";
import type { ReactNode, Ref } from "react";
import { useTranslation } from "react-i18next";
import { focusFirst, useModalA11y } from "../../hooks/useModalA11y";
import { useChatObscuringView } from "../../hooks/useChatObscuringView";
import { classNames } from "../../utils/classNames";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { ModalFocusGuard } from "../ModalFocusGuard";
import { ChevronLeftIcon, ChevronRightIcon, CloseIcon, DownloadIcon, LcdIcon } from "../Icons";
import type { AttachmentResourceMedia } from "../resourcePanel/views/AttachmentResourceView";
import { dialogIconKey } from "../ui/toolbarKey";
import { attachmentStepKey } from "./attachmentStepKey";
import "./mediaLightbox.css";

/**
 * Previous or next through the message's attachments. A key at either end stays focusable and
 * in place, so the arrow keys can turn back from it and the other key never moves.
 */
function StepKey({
  direction,
  disabled,
  onStep,
}: {
  direction: "previous" | "next";
  disabled: boolean;
  onStep: (delta: -1 | 1) => void;
}) {
  const { t } = useTranslation("chat");
  const label = t(direction === "previous" ? "attachmentPrevious" : "attachmentNext");
  const Icon = direction === "previous" ? ChevronLeftIcon : ChevronRightIcon;
  return (
    <div data-media-step={direction}>
      <button
        type="button"
        className={`${dialogIconKey} aria-disabled:pointer-events-none aria-disabled:opacity-35`}
        aria-label={label}
        title={label}
        aria-disabled={disabled || undefined}
        onClick={() => {
          if (!disabled) onStep(direction === "previous" ? -1 : 1);
        }}
        data-attachment-step={direction}
        data-lcd-key
      >
        <Icon size={24} strokeWidth={1.25} absoluteStrokeWidth={false} />
      </button>
    </div>
  );
}

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
  const position = stepping?.position;
  // A step replaces the attachment's view; when that view held focus, the
  // browser drops it to the page, where the arrows no longer reach the dialog,
  // so focus goes to the dialog's first control, as when it opened.
  useEffect(() => {
    const modal = modalRef.current;
    if (position === undefined || !modal) return;
    const active = document.activeElement;
    if (active && active !== document.body && active.isConnected) return;
    focusFirst(modal);
  }, [position, modalRef]);
  useChatObscuringView(open);
  const hintId = useId();
  if (!open) return null;
  return (
    // The dialog keeps Tab inside itself (useModalA11y). The portal's own tab-order
    // guards would also pull its keys out of the order while focus leaves a view
    // a step removed, leaving the dialog with nothing to focus.
    <FloatingPortal preserveTabOrder={false}>
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
              <div className="min-w-0">
                <p className="flex min-w-0 items-baseline gap-2">
                  <span
                    className={classNames(
                      "truncate text-sm font-medium",
                      "text-[var(--color-text-primary)]",
                    )}
                  >
                    {title}
                  </span>
                  {stepping ? (
                    <span
                      className="shrink-0 text-xs tabular-nums text-[var(--color-text-tertiary)]"
                      title={t("attachmentPosition", {
                        position: stepping.position + 1,
                        count: stepping.count,
                      })}
                      aria-live="polite"
                      aria-atomic="true"
                      data-attachment-position
                    >
                      {stepping.position + 1}/{stepping.count}
                    </span>
                  ) : null}
                </p>
                <p
                  className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}
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

          {/* Beside the attachment, or below it on a narrow screen, where a video's own controls
              take the bottom corners. */}
          <div
            className="flex min-h-0 flex-1 flex-col"
            data-media-body
            data-stepping={stepping ? "" : undefined}
          >
            {stepping ? (
              <StepKey
                direction="previous"
                disabled={stepping.position <= 0}
                onStep={stepping.onStep}
              />
            ) : null}
            <div className="min-h-0 flex-1" data-media-content>
              {children}
            </div>
            {stepping ? (
              <StepKey
                direction="next"
                disabled={stepping.position >= stepping.count - 1}
                onStep={stepping.onStep}
              />
            ) : null}
          </div>
          <ModalFocusGuard />
        </div>
      </div>
    </FloatingPortal>
  );
}
