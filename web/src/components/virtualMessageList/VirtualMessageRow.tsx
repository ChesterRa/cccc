import { memo, useCallback } from "react";
import type {
  Actor,
  AgentState,
  LedgerEvent,
  PresentationMessageRef,
  Task,
  TaskMessageRef,
} from "../../types";
import type { WebModelDeliveryStatus } from "../../utils/webModelDeliveryStatus";
import { MessageBubble } from "../MessageBubble";

type VirtualMessageRowProps = {
  virtualRow: { key: React.Key; index: number; start: number };
  message: LedgerEvent;
  resolvedReplyQuoteText?: string;
  collapseHeader?: boolean;
  compactSpacing?: boolean;
  actorById: Map<string, Actor>;
  actors: Actor[];
  displayNameMap: Map<string, string>;
  agentState: AgentState | null;
  taskById: Map<string, Task>;
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  webModelDeliveryStatus?: WebModelDeliveryStatus;
  highlightEventId?: string;
  onReply: (event: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  onCopyLink?: (eventId: string) => void;
  onCopyContent?: (event: LedgerEvent) => void;
  onRelay?: (event: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  onOpenReplyTarget?: (replyToEventId: string) => void;
  measureElement: (node: Element | null) => void;
};

export const VirtualMessageRow = memo(function VirtualMessageRow({
  virtualRow,
  message,
  resolvedReplyQuoteText,
  collapseHeader,
  compactSpacing,
  measureElement,
  highlightEventId,
  onReply,
  onShowRecipients,
  ...messageBubbleProps
}: VirtualMessageRowProps) {
  const attachMeasuredRow = useCallback(
    (node: HTMLDivElement | null) => measureElement(node),
    [measureElement],
  );
  return (
    <div
      data-index={virtualRow.index}
      data-message-row="true"
      data-message-id={message.id ? String(message.id) : ""}
      ref={attachMeasuredRow}
      style={{
        position: "absolute",
        top: 0,
        left: 0,
        width: "100%",
        transform: `translateY(${virtualRow.start}px)`,
      }}
      className={compactSpacing ? "pb-3" : "pb-6"}
    >
      <MessageBubble
        {...messageBubbleProps}
        event={message}
        resolvedReplyQuoteText={resolvedReplyQuoteText}
        collapseHeader={collapseHeader}
        isHighlighted={!!highlightEventId && String(message.id || "") === String(highlightEventId)}
        onReply={() => onReply(message)}
        onShowRecipients={() => {
          if (message.id) onShowRecipients(String(message.id));
        }}
      />
    </div>
  );
});
