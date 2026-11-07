import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { fetchTerminalTail } from "./diagnostics";

describe("diagnostics terminal snapshot api", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("returns the raw cursor paired with a rendered terminal snapshot", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({ ok: true, result: { text: "screen", hint: "", end_cursor: 1234 } }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );

    const response = await fetchTerminalTail("g1", "actor-1", 200_000, true, false);

    expect(response.ok).toBe(true);
    if (!response.ok) throw new Error("expected terminal tail response");
    expect(response.result).toMatchObject({ text: "screen", end_cursor: 1234 });
  });
});
