import { afterEach, expect, it, vi } from "vite-plus/test";
import { fetchDotsActorState, updateDotsActorConnection } from "./webModel";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it("uses a read-only Actor-scoped status URL and explicit connection actions", async () => {
  vi.stubGlobal("window", { location: { search: "" } });
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  await fetchDotsActorState("g a", "dot/a");
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/web-model/dots?group_id=g+a&actor_id=dot%2Fa");
  expect(fetch.mock.calls[0][1]?.method).toBeUndefined();
  await updateDotsActorConnection("g a", "dot/a", "resolve", "turn_exact");
  expect(fetch.mock.calls[1][0]).toBe("/api/v1/web-model/dots");
  expect(fetch.mock.calls[1][1]?.method).toBe("POST");
  expect(JSON.parse(String(fetch.mock.calls[1][1]?.body))).toEqual({
    group_id: "g a",
    actor_id: "dot/a",
    action: "resolve",
    turn_id: "turn_exact",
  });
});
