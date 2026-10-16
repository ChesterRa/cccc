import { afterEach, expect, it, vi } from "vite-plus/test";
import { searchChatMessages } from "./messaging";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("encodes the sender collection as one read with an unchanged shared history cursor", async () => {
  vi.stubGlobal("window", { location: { search: "" } });
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(
      new Response(
        JSON.stringify({ ok: true, result: { events: [], count: 0, has_more: false } }),
        { status: 200 },
      ),
    );
  await searchChatMessages("g1", "", { byAny: ["管理员", "peer,2"], before: "event-1", limit: 50 });
  expect(fetch).toHaveBeenCalledTimes(1);
  const url = new URL(String(fetch.mock.calls[0][0]), "http://localhost");
  expect(JSON.parse(url.searchParams.get("by_any")!)).toEqual(["管理员", "peer,2"]);
  expect(url.searchParams.has("by")).toBe(false);
  expect(url.searchParams.get("before")).toBe("event-1");
  expect(url.searchParams.get("kind")).toBe("chat");
  expect(fetch.mock.calls[0][1]?.method ?? "GET").toBe("GET");
});
