import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../services/api";
import type { LedgerEvent } from "../../types";
import type { GroupChatBucket } from "../../stores/groupStoreCore";
import { reconcileLedgerTail } from "./reconcileLedgerTail";

const testState = vi.hoisted(() => ({
  chatByGroup: {} as Record<
    string,
    Pick<GroupChatBucket, "events"> & Partial<Pick<GroupChatBucket, "senderHistory">>
  >,
  setEvents: vi.fn(),
  setHasMoreHistory: vi.fn(),
  promoteStreamingEventsByPrefix: vi.fn(),
  mergeEventStatuses: vi.fn(),
}));

vi.mock("../../services/api", () => ({
  fetchLedgerBoundary: vi.fn(),
  fetchLedgerStatuses: vi.fn(),
  fetchLedgerTail: vi.fn(),
  searchChatMessages: vi.fn(),
}));

vi.mock("../../stores", () => ({ useGroupStore: { getState: () => testState } }));

vi.mock("../../utils/chatOutboxReconciliation", () => ({
  reconcileCanonicalOutboxEvent: (event: LedgerEvent) => ({ event, clientId: "" }),
  completeCanonicalOutboxReconciliation: vi.fn(),
}));

function event(id: string, kind = "chat.message"): LedgerEvent {
  return {
    v: 1,
    id,
    ts: "2026-08-26T00:00:00Z",
    kind,
    group_id: "g-test",
    scope_key: "",
    by: "user",
    data: { text: id },
  } as LedgerEvent;
}

describe("reconcileLedgerTail", () => {
  const fetchBoundary = vi.mocked(api.fetchLedgerBoundary);
  const fetchStatuses = vi.mocked(api.fetchLedgerStatuses);
  const fetchTail = vi.mocked(api.fetchLedgerTail);
  const searchMessages = vi.mocked(api.searchChatMessages);

  beforeEach(() => {
    vi.clearAllMocks();
    testState.chatByGroup = { "g-test": { events: [event("anchor")] } };
    testState.setEvents.mockImplementation((events: LedgerEvent[], groupId: string) => {
      testState.chatByGroup[groupId] = { ...testState.chatByGroup[groupId], events };
    });
    fetchStatuses.mockResolvedValue({ ok: true, result: { statuses: {} } });
  });

  it("pages forward from the exact ledger boundary instead of keeping only the latest tail", async () => {
    const firstPage = Array.from({ length: 200 }, (_, index) => event(`event-${index + 1}`));
    const secondPage = Array.from({ length: 5 }, (_, index) => event(`event-${index + 201}`));
    fetchBoundary.mockResolvedValue({
      ok: true,
      result: { events: [event("boundary", "runtime.delivery")], has_more: true, count: 1 },
    });
    searchMessages
      .mockResolvedValueOnce({
        ok: true,
        result: { events: firstPage, has_more: true, count: firstPage.length },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: { events: secondPage, has_more: false, count: secondPage.length },
      });

    const cursor = await reconcileLedgerTail("g-test", () => true, "anchor");

    expect(cursor).toBe("event-205");
    expect(searchMessages).toHaveBeenNthCalledWith(1, "g-test", "", {
      after: "anchor",
      limit: 200,
      includeStatuses: false,
    });
    expect(searchMessages).toHaveBeenNthCalledWith(2, "g-test", "", {
      after: "event-200",
      limit: 200,
      includeStatuses: false,
    });
    expect(fetchTail).not.toHaveBeenCalled();
    expect(testState.chatByGroup["g-test"].events).toHaveLength(206);
    expect(testState.chatByGroup["g-test"].events.at(-1)?.id).toBe("event-205");
  });

  it("falls back to a fresh bounded snapshot when the saved cursor no longer exists", async () => {
    fetchBoundary.mockResolvedValue({
      ok: true,
      result: { events: [event("latest-fact", "actor.activity")], has_more: true, count: 1 },
    });
    searchMessages.mockResolvedValue({
      ok: false,
      error: { code: "event_not_found", message: "cursor expired" },
    });
    fetchTail.mockResolvedValue({
      ok: true,
      result: { events: [event("tail-1"), event("tail-2")], has_more: true, count: 2 },
    });

    const cursor = await reconcileLedgerTail("g-test", () => true, "expired");

    expect(cursor).toBe("latest-fact");
    expect(testState.setHasMoreHistory).toHaveBeenCalledWith(true, "g-test");
    expect(testState.chatByGroup["g-test"].events.map((item) => item.id)).toContain("tail-2");
  });

  it("refreshes receipts for sender-only rows as well as the raw reconnect tail", async () => {
    testState.chatByGroup["g-test"].senderHistory = {
      key: "filter",
      senders: ["user"],
      events: [event("private-old"), event("anchor")],
      messageRevision: 0,
      receiptRevision: 0,
    };
    fetchBoundary.mockResolvedValue({
      ok: true,
      result: { events: [event("anchor")], has_more: true, count: 1 },
    });
    await reconcileLedgerTail("g-test", () => true, "anchor");
    expect(fetchStatuses).toHaveBeenCalledWith("g-test", ["anchor", "private-old"]);
  });

  it("keeps reconnect receipt batches within the daemon's 1,000-ID limit", async () => {
    const privateEvents = Array.from({ length: 1001 }, (_, index) => event(`private-${index}`));
    testState.chatByGroup["g-test"].senderHistory = {
      key: "filter",
      senders: ["user"],
      events: privateEvents,
      messageRevision: 0,
      receiptRevision: 0,
    };
    fetchBoundary.mockResolvedValue({
      ok: true,
      result: { events: [event("anchor")], has_more: true, count: 1 },
    });
    await reconcileLedgerTail("g-test", () => true, "anchor");
    expect(fetchStatuses).toHaveBeenCalledTimes(2);
    const requested = fetchStatuses.mock.calls.flatMap((call) => call[1]);
    expect(requested).toEqual(["anchor", ...privateEvents.map((event) => event.id)]);
    expect(fetchStatuses.mock.calls.every((call) => call[1].length <= 1000)).toBe(true);
  });
});
