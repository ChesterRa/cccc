import { useTranslation } from "react-i18next";
import { ChevronLeftIcon, ChevronRightIcon } from "../Icons";

/**
 * Previous and next through a message's attachments, with where this one stands, in the look of
 * the terminal pages' keys. Each end stays focusable, so the arrow keys can turn back from it.
 */
export function AttachmentStepper({
  position,
  count,
  onStep,
  keyClassName,
  iconSize,
}: {
  position: number;
  count: number;
  onStep: (delta: -1 | 1) => void;
  /** The host bar's own key, so the two keys match its others. */
  keyClassName: string;
  iconSize: number;
}) {
  const { t } = useTranslation("chat");
  const first = position <= 0;
  const last = position >= count - 1;
  const key = `${keyClassName} aria-disabled:pointer-events-none aria-disabled:opacity-35`;
  return (
    <span className="flex shrink-0 items-center text-xs" data-attachment-stepper>
      <button
        type="button"
        className={key}
        aria-label={t("attachmentPrevious")}
        title={t("attachmentPrevious")}
        aria-disabled={first || undefined}
        onClick={() => {
          if (!first) onStep(-1);
        }}
        data-attachment-step="previous"
        data-lcd-key
      >
        <ChevronLeftIcon size={iconSize} strokeWidth={1.25} absoluteStrokeWidth={false} />
      </button>
      <span
        aria-live="polite"
        aria-atomic="true"
        className="min-w-8 text-center tabular-nums text-[var(--color-text-secondary)]"
        data-attachment-position
      >
        {position + 1}/{count}
      </span>
      <button
        type="button"
        className={key}
        aria-label={t("attachmentNext")}
        title={t("attachmentNext")}
        aria-disabled={last || undefined}
        onClick={() => {
          if (!last) onStep(1);
        }}
        data-attachment-step="next"
        data-lcd-key
      >
        <ChevronRightIcon size={iconSize} strokeWidth={1.25} absoluteStrokeWidth={false} />
      </button>
    </span>
  );
}
