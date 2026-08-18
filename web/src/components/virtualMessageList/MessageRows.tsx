import type { MutableRefObject } from "react";
import type { Actor, AgentState, LedgerEvent } from "../../types";
import { MessageBubble } from "../MessageBubble";
import { getReplyQuoteText, getStableMessageKey } from "../virtualMessageListHelpers";
import { VirtualMessageRow } from "./VirtualMessageRow";
import { getMessageRowGrouping } from "./grouping";
import type { VirtualMessageListProps } from "./types";

type MessageRowsProps = Pick<
  VirtualMessageListProps,
  | "actors"
  | "taskById"
  | "isDark"
  | "readOnly"
  | "groupId"
  | "groupLabelById"
  | "webModelDeliveryStatusByEventId"
  | "onReply"
  | "onShowRecipients"
  | "onCopyLink"
  | "onCopyContent"
  | "onRelay"
  | "onOpenSource"
  | "onOpenPresentationRef"
  | "onOpenTaskRef"
> & {
  messages: LedgerEvent[];
  shouldVirtualize: boolean;
  virtualItems: { key: React.Key; index: number; start: number }[];
  totalVirtualSize: number;
  nonVirtualTopMargin: number;
  contentRef: MutableRefObject<HTMLDivElement | null>;
  messageTextById: ReadonlyMap<string, string>;
  actorById: Map<string, Actor>;
  agentStateById: Map<string, AgentState>;
  displayNameMap: Map<string, string>;
  effectiveHighlightEventId?: string;
  onOpenReplyTarget: (eventId: string) => void;
  measureElement: (node: Element | null) => void;
};

export function MessageRows({
  messages,
  shouldVirtualize,
  virtualItems,
  totalVirtualSize,
  nonVirtualTopMargin,
  contentRef,
  messageTextById,
  actorById,
  actors,
  agentStateById,
  displayNameMap,
  taskById,
  isDark,
  readOnly,
  groupId,
  groupLabelById,
  webModelDeliveryStatusByEventId,
  effectiveHighlightEventId,
  onReply,
  onShowRecipients,
  onCopyLink,
  onCopyContent,
  onRelay,
  onOpenSource,
  onOpenPresentationRef,
  onOpenTaskRef,
  onOpenReplyTarget,
  measureElement,
}: MessageRowsProps) {
  const common = {
    actorById,
    actors,
    displayNameMap,
    taskById,
    isDark,
    readOnly,
    groupId,
    groupLabelById,
    onCopyLink,
    onCopyContent,
    onRelay,
    onOpenSource,
    onOpenPresentationRef,
    onOpenTaskRef,
    onOpenReplyTarget,
  };
  const deliveryStatus = (message: LedgerEvent) =>
    message.id ? webModelDeliveryStatusByEventId?.[String(message.id)] : undefined;

  if (shouldVirtualize) {
    return (
      <div
        ref={contentRef}
        style={{
          height: `${totalVirtualSize}px`,
          width: "100%",
          position: "relative",
          contain: "layout paint",
        }}
      >
        {virtualItems.map((virtualRow) => {
          const message = messages[virtualRow.index];
          const previous = virtualRow.index > 0 ? messages[virtualRow.index - 1] : undefined;
          const grouping = getMessageRowGrouping(previous, message);
          return (
            <VirtualMessageRow
              {...common}
              key={virtualRow.key}
              virtualRow={virtualRow}
              message={message}
              resolvedReplyQuoteText={getReplyQuoteText(message, messageTextById)}
              collapseHeader={grouping.collapseHeader}
              compactSpacing={grouping.compactSpacing}
              agentState={agentStateById.get(String(message.by || "")) || null}
              webModelDeliveryStatus={deliveryStatus(message)}
              highlightEventId={effectiveHighlightEventId}
              onReply={onReply}
              onShowRecipients={onShowRecipients}
              measureElement={measureElement}
            />
          );
        })}
      </div>
    );
  }

  return (
    <div ref={contentRef} className="w-full" style={{ marginTop: nonVirtualTopMargin }}>
      {messages.map((message, index) => {
        const grouping = getMessageRowGrouping(
          index > 0 ? messages[index - 1] : undefined,
          message,
        );
        return (
          <div
            key={String(getStableMessageKey(message, index))}
            data-index={index}
            data-message-row="true"
            data-message-id={message.id ? String(message.id) : ""}
            className={grouping.compactSpacing ? "pb-3" : "pb-6"}
          >
            <MessageBubble
              {...common}
              event={message}
              resolvedReplyQuoteText={getReplyQuoteText(message, messageTextById)}
              agentState={agentStateById.get(String(message.by || "")) || null}
              webModelDeliveryStatus={deliveryStatus(message)}
              isHighlighted={
                !!effectiveHighlightEventId &&
                String(message.id || "") === String(effectiveHighlightEventId)
              }
              collapseHeader={grouping.collapseHeader}
              onReply={() => onReply(message)}
              onShowRecipients={() => {
                if (message.id) onShowRecipients(String(message.id));
              }}
            />
          </div>
        );
      })}
    </div>
  );
}
