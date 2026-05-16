import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { restartActorFreshSession } from "../../../src/services/api";

function makeJsonResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    text: () => Promise.resolve(JSON.stringify(body)),
  } as Response;
}

beforeEach(() => {
  vi.stubGlobal("sessionStorage", {
    getItem: () => null,
    setItem: () => undefined,
    removeItem: () => undefined,
  });
  vi.stubGlobal("window", {
    location: { search: "" },
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("actor API", () => {
  it("restarts an actor with a fresh session flag", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(makeJsonResponse({ ok: true, result: { actor: { id: "peer1" } } }));
    vi.stubGlobal("fetch", fetchMock);

    await restartActorFreshSession("g1", "peer1");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/api/v1/groups/g1/actors/peer1/restart?by=user");
    expect(JSON.parse(String((init as RequestInit).body || "{}"))).toEqual({ fresh_session: true });
  });
});
