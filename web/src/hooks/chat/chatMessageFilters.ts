import type { ChatFilter } from "../../stores/useUIStore";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { projectMessageMode } from "../../utils/crossGroupRecipients";

export function matchesChatFilters(
  event: LedgerEvent,
  kind: ChatFilter,
  senders: readonly string[],
) {
  if (senders.length > 0 && !senders.includes(event.by ?? "")) return false;
  const data = event.data as ChatMessageData;
  if (kind === "mail" || kind === "request_reply") return projectMessageMode(data) === kind;
  if (kind === "user") {
    if (String(data?.dst_group_id || "").trim()) return false;
    const to = Array.isArray(data?.to) ? data.to : [];
    return event.by === "user" || to.includes("user") || to.includes("@user");
  }
  return true;
}
