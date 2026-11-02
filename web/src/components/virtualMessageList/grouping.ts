import type { ChatMessageData, LedgerEvent } from "../../types";
import { projectMessageMode } from "../../utils/crossGroupRecipients";

// Who wrote a message. Messages relayed from another instance or an IM chat share
// one sender ID per channel, so the author they name, on its platform, and the Group
// a relay came from, tell their writers apart.
function messageAuthor(message: LedgerEvent): string {
  const by = String(message.by || "").trim();
  if (!by) return "";
  const data = message.data as ChatMessageData | undefined;
  const source = [data?.src_group_id, data?.source_platform, data?.source_user_id].map((part) =>
    String(part || "").trim(),
  );
  return source.some(Boolean) ? [by, ...source].join("\u0000") : by;
}

export function getMessageRowGrouping(
  previousMessage: LedgerEvent | undefined,
  message: LedgerEvent | undefined,
): { collapseHeader: boolean; compactSpacing: boolean; sameSender: boolean } {
  const separate = { collapseHeader: false, compactSpacing: false, sameSender: false };
  if (!previousMessage || !message) return separate;
  if (previousMessage.kind !== "chat.message" || message.kind !== "chat.message") {
    return separate;
  }

  const previousSender = messageAuthor(previousMessage);
  const sameSender = !!previousSender && previousSender === messageAuthor(message);
  const previousData = previousMessage.data as ChatMessageData;
  const data = message.data as ChatMessageData;
  const previousMode = projectMessageMode(previousData);
  const mode = projectMessageMode(data);
  const preventsCollapse =
    !sameSender ||
    previousMode !== mode ||
    mode === "request_reply" ||
    !!data?.reply_to ||
    !previousMessage.ts ||
    !message.ts;
  if (preventsCollapse) return { collapseHeader: false, compactSpacing: false, sameSender };

  const previousTime = new Date(String(previousMessage.ts)).getTime();
  const currentTime = new Date(String(message.ts)).getTime();
  const collapseHeader =
    Number.isFinite(previousTime) &&
    Number.isFinite(currentTime) &&
    Math.abs(currentTime - previousTime) < 3 * 60 * 1000;
  return { collapseHeader, compactSpacing: collapseHeader, sameSender };
}
