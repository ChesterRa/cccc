import { afterEach, expect, it, vi } from "vite-plus/test";
import {
  answerPendingReply,
  fetchPendingReplies,
  fetchPendingReplySummary,
} from "./pendingReplies";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("passes an opaque cursor and an exact answer without changing either", async () => {
  vi.stubGlobal("window", { location: { search: "" } });
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(new Response(JSON.stringify({ ok: true, result: {} }), { status: 200 }));
  await fetchPendingReplies("g/1", "opaque/+?=");
  const page = new URL(String(fetch.mock.calls[0]![0]), "http://localhost");
  expect(page.pathname).toBe("/api/v1/groups/g%2F1/reply-requests/pending");
  expect(page.searchParams.get("before")).toBe("opaque/+?=");
  expect(page.searchParams.get("limit")).toBe("50");

  await answerPendingReply("g/1", "event/1", "stable-id", { v: 1, text: "  🌸\n答复  " });
  const [path, init] = fetch.mock.calls[1]!;
  expect(path).toBe("/api/v1/groups/g%2F1/messages/event%2F1/reply-request/answer");
  expect(init?.method).toBe("POST");
  expect(JSON.parse(String(init?.body))).toEqual({
    client_id: "stable-id",
    answer: { v: 1, text: "  🌸\n答复  " },
  });
});

it("reads a fresh principal-filtered summary without browser caching", async () => {
  vi.stubGlobal("window", { location: { search: "" } });
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(
      new Response(JSON.stringify({ ok: true, result: { groups: [] } }), { status: 200 }),
    );
  await fetchPendingReplySummary();
  expect(fetch.mock.calls[0]![0]).toBe("/api/v1/reply-requests/summary");
  expect(fetch.mock.calls[0]![1]?.cache).toBe("no-store");
});
