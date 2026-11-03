import { usePinStore } from "../../features/pins/pinStore";
import type { ChatFilter } from "../../stores/useUIStore";
import type { ChatMessageData, LedgerEvent } from "../../types";

/** Which optional filter tabs this view offers. */
export type OfferedChatFilters = { waiting: boolean; pinned: boolean };

/**
 * The filter a Group's messages show under. A stored filter whose tab is not offered shows
 * every message: Waiting for you in a read-only exhibit or while its prompts are off, and
 * Pinned in an exhibit or where the daemon keeps no pins.
 */
export function shownChatFilter(stored: ChatFilter, offered: OfferedChatFilters): ChatFilter {
  if (stored === "pending_user" && !offered.waiting) return "all";
  if (stored === "pinned" && !offered.pinned) return "all";
  return stored;
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
  if (kind === "pinned") {
    const groupId = String(event.group_id || "");
    return !!event.id && !!usePinStore.getState().groups[groupId]?.pinned[event.id];
  }
  if (kind === "user") {
    if (String(data?.dst_group_id || "").trim()) return false;
    const to = Array.isArray(data?.to) ? data.to : [];
    return event.by === "user" || to.includes("user") || to.includes("@user");
  }
  return true;
}

/**
 * What the Pinned view says when it lists nothing. "No pinned messages" is only for a Group
 * known to have none: pins that the sender filter hides, pins that can no longer be read
 * (reported above the list), or a count not yet known get the general "No results" instead
 * (undefined here).
 */
export function pinnedEmptyKey(error: boolean, count: number | null): string | undefined {
  if (error) return "pins.loadFailed";
  return count === 0 ? "pins.empty" : undefined;
}
