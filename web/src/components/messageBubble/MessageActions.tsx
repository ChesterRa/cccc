import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LedgerEvent } from "../../types";
import { MoreIcon } from "../Icons";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export interface MessageActionsProps {
  readOnly?: boolean;
  copiedMessageText: boolean;
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
export function MessageActions(props: MessageActionsProps) {
  const { t } = useTranslation(["chat", "common"]);
  const [open, setOpen] = useState(false);
  const replySelected = useRef(false);
  if (props.readOnly) return null;
  const actions: { key: string; label: string; run: () => void }[] = [];
  if (props.copyableMessageText)
    actions.push({
      key: "copy",
      label: props.copiedMessageText ? t("common:copied") : t("copyText"),
      run: props.onCopyMessageText,
    });
  if (props.eventId && props.onViewContext)
    actions.push({
      key: "context",
      label: t("viewMessageContext"),
      run: () => props.onViewContext?.(props.eventId!),
    });
  if (props.eventId && props.onCopyLink)
    actions.push({
      key: "link",
      label: t("copyLink"),
      run: () => props.onCopyLink?.(props.eventId!),
    });
  if (props.eventId && props.onRelay)
    actions.push({ key: "relay", label: t("relay"), run: () => props.onRelay?.(props.event) });
  if (props.canReply) actions.push({ key: "reply", label: t("reply"), run: props.onReply });
  if (!actions.length) return null;
  return (
    <>
      <div
        className={`message-actions-inline flex-wrap items-center gap-0.5 ${props.event.by === "user" ? "justify-end" : "justify-start"}`}
        data-message-actions-inline
      >
        {actions.map((action) => (
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
      </div>
      <div className="message-actions-menu">
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="gap-1 rounded-lg px-2 text-xs"
              aria-label={t("messageActions")}
            >
              <MoreIcon size={14} aria-hidden="true" />
              {t("messageActions")}
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align={props.event.by === "user" ? "end" : "start"}
            side="top"
            className="min-w-36 p-1"
            onCloseAutoFocus={(event) => {
              if (replySelected.current) {
                event.preventDefault();
                replySelected.current = false;
              }
            }}
          >
            <div className="flex flex-col" role="group" aria-label={t("messageActions")}>
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
      </div>
    </>
  );
}
