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
}

export type MessageAction = { key: string; label: string; run: () => void };
type MenuSection = { key: string; label?: string; actions: MessageAction[] };

// Reply, Forward and Copy text stay on the hover bar, Copy text as an icon named by its
// tooltip; quick replies and Copy link wait in its overflow menu. Touch screens list every
// action in one menu. A filtered view puts Show in chat first on the bar; where there is no
// bar, or no room on it (lcd.css), the message head shows it instead (MessageBubble), so no
// menu holds it.
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
  const copy: MessageAction | null = props.copyableMessageText
    ? { key: "copy", label: t("copyText"), run: props.onCopyMessageText }
    : null;
  const overflow: MessageAction[] = [];
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
  const bar = show ? [show, ...primary] : primary;
  const barMenu = [...quick, ...more];
  const touchMenu = [
    { key: "main", actions: [...primary, ...(copy ? [copy] : [])] },
    ...quick,
    ...more,
  ].filter((section) => section.actions.length > 0);
  if (!touchMenu.length && !bar.length) return null;
  const align = props.event.by === "user" ? "end" : "start";
  return (
    <>
      <div
        className={`message-actions-inline flex-wrap items-center gap-0.5 ${props.event.by === "user" ? "justify-end" : "justify-start"}`}
        data-message-actions-inline
      >
        {bar.map((action) => (
          <Button
            key={action.key}
            variant="ghost"
            size="sm"
            className="min-h-7 rounded-lg px-2 py-1 text-xs duration-150"
            data-show-in-chat={action === show || undefined}
            onClick={action.run}
          >
            {action.label}
          </Button>
        ))}
        {copy ? (
          <Button
            variant="ghost"
            size="sm"
            className="min-h-7 rounded-lg px-2 py-1 text-xs duration-150"
            aria-label={copy.label}
            title={copy.label}
            onClick={copy.run}
          >
            <CopyIcon size={14} aria-hidden="true" />
          </Button>
        ) : null}
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
      {touchMenu.length ? (
        <div className="message-actions-menu">
          <MessageActionMenu sections={touchMenu} align={align} label={t("messageActions")}>
            <Button
              variant="ghost"
              size="sm"
              className="gap-1 rounded-lg px-2 text-xs"
              aria-label={t("messageActions")}
            >
              <MoreIcon size={14} aria-hidden="true" />
              <span className="message-actions-label">{t("messageActions")}</span>
            </Button>
          </MessageActionMenu>
        </div>
      ) : null}
    </>
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
              <div
                className="px-3 pb-0.5 pt-1 text-xs text-[var(--color-text-muted)]"
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
                {action.label}
              </Button>
            ))}
          </div>
        ))}
      </PopoverContent>
    </Popover>
  );
}
