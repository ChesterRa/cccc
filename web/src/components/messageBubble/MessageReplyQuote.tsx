import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";

/**
 * The excerpt a reply quotes. It is a short snapshot: when it leads to the
 * replied message, the whole quote is that link and two lines of it show;
 * otherwise all of it shows, since nothing else would.
 */
export function MessageReplyQuote({
  text,
  replyToEventId,
  onOpenReplyTarget,
}: {
  text: string;
  replyToEventId?: string;
  onOpenReplyTarget?: (id: string) => void;
}) {
  const { t } = useTranslation("chat");
  const quoteClass =
    "mb-3 block w-full min-w-0 border-l-2 border-[var(--color-border-primary)] py-1 pl-3 text-left text-xs text-[var(--color-text-secondary)]";
  const content = (clamped: boolean) => (
    <span
      className={classNames(
        "min-w-0 whitespace-pre-wrap break-words leading-5 [overflow-wrap:anywhere]",
        clamped && "line-clamp-2",
      )}
    >
      {text}
    </span>
  );
  if (!replyToEventId || !onOpenReplyTarget) {
    return (
      <div className={quoteClass} data-message-quote>
        {content(false)}
      </div>
    );
  }
  return (
    <button
      type="button"
      className={classNames(
        quoteClass,
        "rounded-r-sm transition-colors hover:border-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2",
      )}
      title={t("jumpToRepliedMessage")}
      data-message-quote
      onClick={(event) => {
        event.stopPropagation();
        onOpenReplyTarget(replyToEventId);
      }}
    >
      <span className="sr-only">{t("jumpToRepliedMessage")}: </span>
      {content(true)}
    </button>
  );
}
