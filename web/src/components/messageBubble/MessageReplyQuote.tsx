import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, ReplyIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { classNames } from "../../utils/classNames";

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
  const [expanded, setExpanded] = useState(false);
  const contentId = useId();
  return (
    <div
      className="mb-3 min-w-0 border-l-2 border-[var(--color-border-primary)] pl-3 text-xs text-[var(--color-text-secondary)]"
      data-message-quote
    >
      <div className="flex items-start gap-2">
        <button
          type="button"
          className="flex min-w-0 flex-1 items-start gap-2 rounded-sm py-1 text-left hover:text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2"
          aria-expanded={expanded}
          aria-controls={contentId}
          aria-label={t(expanded ? "collapseQuotedMessage" : "expandQuotedMessage")}
          onClick={(event) => {
            event.stopPropagation();
            setExpanded(!expanded);
          }}
        >
          <span
            id={contentId}
            className={classNames(
              "min-w-0 whitespace-pre-wrap break-words leading-5 [overflow-wrap:anywhere]",
              !expanded && "line-clamp-2",
            )}
          >
            {text}
          </span>
          <ChevronDownIcon
            size={12}
            className={classNames(
              "mt-1 shrink-0 transition-transform duration-150 motion-reduce:transition-none",
              expanded && "rotate-180",
            )}
          />
        </button>
        {replyToEventId && onOpenReplyTarget && (
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            label={t("jumpToRepliedMessage")}
            onClick={(event) => {
              event.stopPropagation();
              onOpenReplyTarget(replyToEventId);
            }}
          >
            <ReplyIcon size={13} />
          </IconButton>
        )}
      </div>
    </div>
  );
}
