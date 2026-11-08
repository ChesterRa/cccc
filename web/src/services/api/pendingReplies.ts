import type { LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { apiJson } from "./base";

export type PendingReplySummary = {
  group_id: string;
  state: "fresh" | "unknown";
  pending: number | null;
  oldest_ts?: string | null;
  revision?: string;
  as_of?: string;
  error?: unknown;
};

export type PendingReplyPage = {
  events: LedgerEvent[];
  statuses: Record<string, LedgerEventStatusPayload>;
  total: number;
  has_more: boolean;
  before?: string | null;
};

/** A choice, other words, or (version 2) a choice with the user's note added. */
export type PendingReplyAnswer =
  | { v: 1; option_id: string }
  | { v: 1; text: string }
  | { v: 2; option_id: string; note: string };

export function fetchPendingReplySummary(init?: RequestInit) {
  return apiJson<{ groups: PendingReplySummary[] }>("/api/v1/reply-requests/summary", {
    ...init,
    cache: "no-store",
  });
}

export function fetchPendingReplies(groupId: string, before?: string, init?: RequestInit) {
  const query = new URLSearchParams({ limit: "50" });
  if (before) query.set("before", before);
  return apiJson<PendingReplyPage>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/reply-requests/pending?${query}`,
    { ...init, cache: "no-store" },
  );
}

export function answerPendingReply(
  groupId: string,
  eventId: string,
  clientId: string,
  answer: PendingReplyAnswer,
  init?: RequestInit,
) {
  return apiJson<{ event: LedgerEvent; duplicate?: boolean }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/messages/${encodeURIComponent(eventId)}/reply-request/answer`,
    { ...init, method: "POST", body: JSON.stringify({ client_id: clientId, answer }) },
  );
}
