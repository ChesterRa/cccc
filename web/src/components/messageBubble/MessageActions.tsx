import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { LedgerEvent } from "../../types";
import { CopyIcon, MoreHorizontalIcon, MoreIcon } from "../Icons";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export interface MessageActionsProps {
  readOnly?: boolean;
  copyableMessageText: string;
  onCopyMessageText: () => void;
  onCopyLink?: (id: string) => void;
  onViewContext?: (id: string) => void;
  onRelay?: (event: LedgerEvent) => void;
  onReply: () => void;
  canReply: boolean;
  eventId?: string;
  event: LedgerEvent;
}

type MessageAction = { key: string; label: string; run: () => void };

// Reply, Forward and Copy text stay on the hover bar, Copy text as an icon named by its
// tooltip; Copy link, and the context jump a filter adds, wait in its overflow menu. Touch
// screens list every action in one menu.
export function MessageActions(props: MessageActionsProps) {
  const { t } = useTranslation(["chat", "common"]);
  if (props.readOnly) return null;
  const primary: MessageAction[] = [];
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
  if (props.eventId && props.onViewContext)
    overflow.push({
      key: "context",
      label: t("viewMessageContext"),
      run: () => props.onViewContext?.(props.eventId!),
    });
  if (!primary.length && !copy && !overflow.length) return null;
  const align = props.event.by === "user" ? "end" : "start";
  return (
    <>
      <div
        className={`message-actions-inline flex-wrap items-center gap-0.5 ${props.event.by === "user" ? "justify-end" : "justify-start"}`}
        data-message-actions-inline
      >
        {primary.map((action) => (
          <Button
            key={action.key}
            variant="ghost"
            size="sm"
            className="min-h-7 rounded-lg px-2 py-1 text-xs duration-150"
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
        {overflow.length ? (
          <MessageActionMenu actions={overflow} align={align} label={t("messageActions")}>
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
      <div className="message-actions-menu">
        <MessageActionMenu
          actions={[...primary, ...(copy ? [copy] : []), ...overflow]}
          align={align}
          label={t("messageActions")}
        >
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
    </>
  );
}

function MessageActionMenu({
  actions,
  align,
  label,
  children,
}: {
  actions: MessageAction[];
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
        <div className="flex flex-col" role="group" aria-label={label}>
          {actions.map((action) => (
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
      </PopoverContent>
    </Popover>
  );
}
