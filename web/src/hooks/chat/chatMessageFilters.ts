import type { ChatFilter } from "../../stores/useUIStore";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { projectMessageMode } from "../../utils/crossGroupRecipients";

/**
 * The filter a Group's messages show under. A stored Waiting for you filter shows every
 * message where its tab is not offered: in a read-only exhibit, or while those prompts are off.
 */
export function shownChatFilter(stored: ChatFilter, waitingTabShown: boolean): ChatFilter {
  return stored === "pending_user" && !waitingTabShown ? "all" : stored;
}

export function matchesChatFilters(
  event: LedgerEvent,
  kind: ChatFilter,
  senders: readonly string[],
) {
  if (senders.length > 0 && !senders.includes(event.by ?? "")) return false;
  const data = event.data as ChatMessageData;
  if (kind === "pending_user") {
    const obligation = event._obligation_status?.user;
    return (
      !!event._user_request &&
      obligation?.reply_requested === true &&
      obligation.replied === false &&
      obligation.cancelled === false
    );
  }
  if (kind === "mail" || kind === "request_reply") return projectMessageMode(data) === kind;
  if (kind === "user") {
    if (String(data?.dst_group_id || "").trim()) return false;
    const to = Array.isArray(data?.to) ? data.to : [];
    return event.by === "user" || to.includes("user") || to.includes("@user");
  }
  return true;
}
