import type { HeadlessStreamEvent, LedgerEvent, LedgerEventStatusPayload } from "../../types";
import { apiJson, ledgerStatusesRequestKey, reuseRecentReadRequest } from "./base";

const LEDGER_STATUSES_TTL_MS = 1200;

type LedgerFetchInit = RequestInit & { includeStatuses?: boolean };

function buildLedgerStatusParams(includeStatuses: boolean): URLSearchParams {
  const params = new URLSearchParams();
  if (includeStatuses) {
    params.set("with_read_status", "true");
    params.set("with_obligation_status", "true");
  }
  return params;
}

export async function fetchLedgerTail(groupId: string, lines = 120, init?: LedgerFetchInit) {
  const includeStatuses = init?.includeStatuses !== false;
  const params = buildLedgerStatusParams(includeStatuses);
  params.set("kind", "chat");
  params.set("limit", String(lines));
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/tail?${params.toString()}`,
    init,
  );
}

export async function fetchLedgerBoundary(groupId: string) {
  const params = new URLSearchParams({ kind: "all", limit: "1" });
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/tail?${params.toString()}`,
    { cache: "no-store" },
  );
}

export async function fetchOlderMessages(groupId: string, beforeEventId: string, limit = 50) {
  const params = new URLSearchParams({
    kind: "chat",
    before: beforeEventId,
    limit: String(limit),
    with_read_status: "true",
    with_obligation_status: "true",
  });
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`,
  );
}

export async function fetchMessageWindow(
  groupId: string,
  centerEventId: string,
  opts?: { before?: number; after?: number },
) {
  const params = new URLSearchParams({
    kind: "chat",
    center: centerEventId,
    before: String(opts?.before ?? 30),
    after: String(opts?.after ?? 30),
    with_read_status: "true",
    with_obligation_status: "true",
  });
  return apiJson<{
    center_id: string;
    center_index: number;
    events: LedgerEvent[];
    has_more_before: boolean;
    has_more_after: boolean;
    count: number;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/window?${params.toString()}`);
}

export async function searchChatMessages(
  groupId: string,
  q: string,
  opts?: { limit?: number; before?: string; after?: string; includeStatuses?: boolean },
) {
  const params = buildLedgerStatusParams(opts?.includeStatuses !== false);
  params.set("kind", "chat");
  params.set("q", q || "");
  params.set("limit", String(opts?.limit ?? 50));
  if (opts?.before) params.set("before", opts.before);
  if (opts?.after) params.set("after", opts.after);
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`,
  );
}

export async function fetchLedgerStatuses(
  groupId: string,
  eventIds: string[],
  init?: RequestInit & { noCache?: boolean },
) {
  const normalizedIds = eventIds
    .map((eventId) => String(eventId || "").trim())
    .filter((eventId) => eventId);
  if (normalizedIds.length === 0) {
    return { ok: true, result: { statuses: {} } } as const;
  }
  const loader = () =>
    apiJson<{ statuses: Record<string, LedgerEventStatusPayload> }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/statuses`,
      { method: "POST", body: JSON.stringify({ event_ids: normalizedIds }), ...init },
    );
  if (init?.noCache || init?.signal) {
    return loader();
  }
  return reuseRecentReadRequest(
    ledgerStatusesRequestKey(groupId, normalizedIds),
    LEDGER_STATUSES_TTL_MS,
    loader,
  );
}

export async function fetchHeadlessSnapshot(
  groupId: string,
  init?: RequestInit & { noCache?: boolean },
) {
  return apiJson<{ group_id: string; events: HeadlessStreamEvent[]; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/headless/snapshot`,
    init,
  );
}
