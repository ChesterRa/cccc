import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { LedgerEvent } from "../../types";
import { CopyIcon, MoreHorizontalIcon, MoreIcon } from "../Icons";
import { Button } from "../ui/button";
import { classNames } from "../../utils/classNames";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export interface MessageActionsProps {
  readOnly?: boolean;
  copyableMessageText: string;
  onCopyMessageText: () => void;
  onCopyLink?: (id: string) => void;
  /** A filtered view's way into the whole conversation; first on the bar. */
  onShowInConversation?: (id: string) => void;
  onRelay?: (event: LedgerEvent) => void;
  onReply: () => void;
  canReply: boolean;
  eventId?: string;
  event: LedgerEvent;
  /** Ready replies sent to the message's author in one choice; none where they do not apply. */
  quickReplies?: MessageAction[];
  /** Whether the message is pinned, with the way to change it; absent where pins are not offered. */
  pinned?: boolean;
  onTogglePin?: () => void;
  /** "bar" for a pointer, shown under the message on hover or focus; "menu" for touch, ⋮ at
   *  the end of the message's head. CSS shows the one that fits the device. */
  part: "bar" | "menu";
}

export type MessageAction = { key: string; label: string; emoji?: string; run: () => void };
type MenuSection = { key: string; label?: string; actions: MessageAction[] };

// With a pointer, Reply, Forward and Copy (an icon named by its tip) show under the message on
// hover or focus, with quick replies, Pin and Copy link in its ⋯ menu, and a filtered view's
// Show in chat first; nothing is reserved for them. Touch lists every action in the ⋮ that
// ends the message's head, where Show in chat follows the time (MessageBubble).
export function MessageActions(props: MessageActionsProps) {
  const { t } = useTranslation(["chat", "common"]);
  if (props.readOnly) return null;
  const primary: MessageAction[] = [];
  const show: MessageAction | null =
    props.eventId && props.onShowInConversation
      ? {
          key: "show",
          label: t("showInConversation"),
          run: () => props.onShowInConversation?.(props.eventId!),
        }
      : null;
  if (props.canReply) primary.push({ key: "reply", label: t("reply"), run: props.onReply });
  if (props.eventId && props.onRelay)
    primary.push({ key: "relay", label: t("relay"), run: () => props.onRelay?.(props.event) });
  if (props.copyableMessageText)
    primary.push({ key: "copy", label: t("copyText"), run: props.onCopyMessageText });
  const overflow: MessageAction[] = [];
  if (props.eventId && props.onTogglePin)
    overflow.push({
      key: "pin",
      label: t(props.pinned ? "pins.unpin" : "pins.pin"),
      run: props.onTogglePin,
    });
  if (props.eventId && props.onCopyLink)
    overflow.push({
      key: "link",
      label: t("copyLink"),
      run: () => props.onCopyLink?.(props.eventId!),
    });
  const quick: MenuSection[] = props.quickReplies?.length
    ? [{ key: "quick", label: t("quickReply.menu"), actions: props.quickReplies }]
    : [];
  const more: MenuSection[] = overflow.length ? [{ key: "more", actions: overflow }] : [];
  const align = props.event.by === "user" ? "end" : "start";
  if (props.part === "menu") {
    const touchMenu = [{ key: "main", actions: primary }, ...quick, ...more].filter(
      (section) => section.actions.length > 0,
    );
    if (!touchMenu.length) return null;
    return (
      <div className="message-actions-menu">
        <MessageActionMenu sections={touchMenu} align={align} label={t("messageActions")}>
          <Button
            variant="ghost"
            size="sm"
            className="rounded-lg px-2 text-xs"
            aria-label={t("messageActions")}
            title={t("messageActions")}
          >
            <MoreIcon size={14} aria-hidden="true" />
          </Button>
        </MessageActionMenu>
      </div>
    );
  }
  const bar = show ? [show, ...primary] : primary;
  const barMenu = [...quick, ...more];
  if (!bar.length && !barMenu.length) return null;
  return (
    <div
      className={`message-actions-inline flex-wrap items-center gap-0.5 ${align === "end" ? "justify-end" : "justify-start"}`}
      data-message-actions-inline
      data-message-actions-align={align}
    >
      {bar.map((action) => (
        <Button
          key={action.key}
          variant="ghost"
          size="sm"
          className="min-h-7 rounded-lg px-2 py-1 text-xs duration-150"
          data-show-in-chat={action === show || undefined}
          aria-label={action.key === "copy" ? action.label : undefined}
          title={action.key === "copy" ? action.label : undefined}
          onClick={action.run}
        >
          {action.key === "copy" ? <CopyIcon size={14} aria-hidden="true" /> : action.label}
        </Button>
      ))}
      {barMenu.length ? (
        <MessageActionMenu sections={barMenu} align={align} label={t("messageActions")}>
          <Button
            variant="ghost"
            size="sm"
            className="min-h-7 rounded-lg px-2 py-1 text-xs duration-150"
            aria-label={t("messageActions")}
            title={t("messageActions")}
          >
            <MoreHorizontalIcon size={14} aria-hidden="true" />
          </Button>
        </MessageActionMenu>
      ) : null}
    </div>
  );
}

function MessageActionMenu({
  sections,
  align,
  label,
  children,
}: {
  sections: MenuSection[];
  align: "start" | "end";
  label: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const replySelected = useRef(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        align={align}
        side="top"
        className="min-w-36 p-1"
        onCloseAutoFocus={(event) => {
          // Reply moves focus to the composer; keep it there.
          if (replySelected.current) {
            event.preventDefault();
            replySelected.current = false;
          }
        }}
      >
        {sections.map((section, index) => (
          <div
            key={section.key}
            className={classNames(
              "flex flex-col",
              index > 0 && "mt-1 border-t border-[var(--glass-border-subtle)] pt-1",
            )}
            role="group"
            aria-label={section.label || label}
          >
            {section.label ? (
              // A caption for the actions below it, set smaller than they are so it does not
              // read as one of them, and kept legible at the smallest text size.
              <div
                className="px-3 pt-1 text-[length:max(0.6875rem,9px)] leading-4 text-[var(--color-text-muted)]"
                aria-hidden="true"
              >
                {section.label}
              </div>
            ) : null}
            {section.actions.map((action) => (
              <Button
                key={action.key}
                variant="ghost"
                size="sm"
                className="justify-start rounded-lg px-3"
                onClick={() => {
                  replySelected.current = action.key === "reply";
                  setOpen(false);
                  action.run();
                }}
              >
                {action.emoji ? (
                  <span className="shrink-0" aria-hidden="true" data-action-emoji>
                    {action.emoji}
                  </span>
                ) : null}
                {action.label}
              </Button>
            ))}
          </div>
        ))}
      </PopoverContent>
    </Popover>
  );
}
