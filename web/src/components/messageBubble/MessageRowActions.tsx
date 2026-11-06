import type { ReactNode } from "react";
import { useUiStylePrefs } from "../../utils/uiStyle";
import { MessageActions, type MessageActionsProps } from "./MessageActions";

type Actions = Omit<MessageActionsProps, "part">;

export function MessageHeadActions({ actions }: { actions: Actions }) {
  const isLcd = useUiStylePrefs().style === "lcd";
  if (!isLcd) return null;
  return (
    <>
      <MessageActions part="bar" {...actions} />
      <MessageActions part="menu" {...actions} />
    </>
  );
}

/** In the classic layout, actions and delivery status share the footer. */
export function MessageRowFooter({ actions, status }: { actions: Actions; status: ReactNode }) {
  const isLcd = useUiStylePrefs().style === "lcd";
  if (isLcd) return status;
  const fromReader = actions.event.by === "user" && !actions.readOnly;
  return (
    <div className="classic-message-footer" data-reader={fromReader || undefined}>
      <div className="classic-message-actions">
        <MessageActions part="bar" {...actions} />
        <MessageActions part="menu" {...actions} />
      </div>
      <div className="classic-message-status">{status}</div>
    </div>
  );
}
