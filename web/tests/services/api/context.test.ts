import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchGroupCapabilityState } from "../../../src/services/api";

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

describe("fetchGroupCapabilityState", () => {
  it("shares concurrent noCache state reads for the same scope", async () => {
    let resolveFetch: ((response: Response) => void) | null = null;
    const fetchPromise = new Promise<Response>((resolve) => {
      resolveFetch = resolve;
    });
    const fetchMock = vi.fn(() => fetchPromise);
    vi.stubGlobal("fetch", fetchMock);

    const first = fetchGroupCapabilityState("g1", "user", { noCache: true });
    const second = fetchGroupCapabilityState("g1", "user", { noCache: true });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    resolveFetch?.(makeJsonResponse({ ok: true, result: { enabled_capabilities: ["skill:a"] } }));
    await expect(Promise.all([first, second])).resolves.toEqual([
      { ok: true, result: { enabled_capabilities: ["skill:a"] } },
      { ok: true, result: { enabled_capabilities: ["skill:a"] } },
    ]);
  });

  it("does not share state reads across capability-specific scopes", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(makeJsonResponse({ ok: true, result: { enabled_capabilities: [] } }));
    vi.stubGlobal("fetch", fetchMock);

    await Promise.all([
      fetchGroupCapabilityState("g2", "user", { capabilityId: "skill:a", noCache: true }),
      fetchGroupCapabilityState("g2", "user", { capabilityId: "skill:b", noCache: true }),
    ]);

    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
