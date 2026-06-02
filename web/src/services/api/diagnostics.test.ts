import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchTerminalHistory, fetchTerminalSnapshot } from "./diagnostics";

describe("diagnostics terminal history api", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("requests latest terminal history page with cursor options", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ok: true,
        result: { text: "tail", start_cursor: 4, end_cursor: 8, has_more: true, cursor_expired: false },
      }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );

    const resp = await fetchTerminalHistory("g 1", "actor/1", {
      before: 12,
      limitBytes: 4096,
      stripAnsi: false,
      compact: false,
    });

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g%201/terminal/history?");
    expect(url).toContain("actor_id=actor%2F1");
    expect(url).toContain("before=12");
    expect(url).toContain("limit_bytes=4096");
    expect(url).toContain("strip_ansi=false");
    expect(url).toContain("compact=false");
  });

  it("requests rendered terminal snapshot for attach cursor", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ok: true,
        result: { text: "screen", start_cursor: 0, end_cursor: 12, cursor_expired: false },
      }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );

    const resp = await fetchTerminalSnapshot("g 1", "actor/1", 8192);

    expect(resp.ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g%201/terminal/snapshot?");
    expect(url).toContain("actor_id=actor%2F1");
    expect(url).toContain("limit_bytes=8192");
  });
});
