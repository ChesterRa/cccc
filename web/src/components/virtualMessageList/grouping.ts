import type { ChatMessageData, LedgerEvent } from "../../types";

export function getMessageRowGrouping(
  previousMessage: LedgerEvent | undefined,
  message: LedgerEvent | undefined,
): { collapseHeader: boolean; compactSpacing: boolean } {
  if (!previousMessage || !message) return { collapseHeader: false, compactSpacing: false };
  if (previousMessage.kind !== "chat.message" || message.kind !== "chat.message") {
    return { collapseHeader: false, compactSpacing: false };
  }

  const previousSender = String(previousMessage.by || "").trim();
  const sender = String(message.by || "").trim();
  const previousData = previousMessage.data as ChatMessageData;
  const data = message.data as ChatMessageData;
  const preventsCollapse =
    !previousSender ||
    previousSender !== sender ||
    previousData?.priority === "attention" ||
    data?.priority === "attention" ||
    !!previousData?.reply_required ||
    !!data?.reply_required ||
    !!data?.reply_to ||
    !previousMessage.ts ||
    !message.ts;
  if (preventsCollapse) return { collapseHeader: false, compactSpacing: false };

  const previousTime = new Date(String(previousMessage.ts)).getTime();
  const currentTime = new Date(String(message.ts)).getTime();
  const collapseHeader =
    Number.isFinite(previousTime) &&
    Number.isFinite(currentTime) &&
    Math.abs(currentTime - previousTime) < 3 * 60 * 1000;
  return { collapseHeader, compactSpacing: collapseHeader };
}
