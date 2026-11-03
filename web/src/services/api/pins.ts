import type { LedgerEvent } from "../../types";
import { apiJson } from "./base";

/** A pinned message; `event` is null once the message can no longer be read. */
export type PinnedMessage = { event_id: string; pinned_at: string; event: LedgerEvent | null };

export type PinnedId = { event_id: string; pinned_at: string };

function pinsPath(groupId: string, detail: "ids" | "full") {
  const path = `/api/v1/groups/${encodeURIComponent(groupId)}/pins`;
  return detail === "ids" ? `${path}?detail=ids` : path;
}

/** The Group's pins in ledger order of their messages; unreadable ones come last. */
export function fetchPinnedIds(groupId: string) {
  return apiJson<{ pins: PinnedId[] }>(pinsPath(groupId, "ids"), { cache: "no-store" });
}

export function fetchPinnedMessages(groupId: string) {
  return apiJson<{ pins: PinnedMessage[] }>(pinsPath(groupId, "full"), { cache: "no-store" });
}

/** Pin or unpin one message; repeating a request that is already in effect succeeds. */
export function setMessagePinned(groupId: string, eventId: string, pinned: boolean) {
  return apiJson<{ pins: PinnedId[] }>(pinsPath(groupId, "ids"), {
    method: "POST",
    body: JSON.stringify({ event_id: eventId, pinned }),
  });
}
