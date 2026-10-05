import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { createDirectory, fetchRuntimes } from "./system";

describe("Runtime detection API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  it("uses a read-only check and retains distinct mode availability", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const result = {
      available: [],
      runtimes: [
        {
          name: "antigravity",
          display_name: "Antigravity",
          available: false,
          mode_availability: { default: false, acp: true },
        },
      ],
    };
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ ok: true, result }), {
          headers: { "content-type": "application/json" },
        }),
      );
    expect(await fetchRuntimes()).toEqual({ ok: true, result });
    expect(fetch.mock.calls[0]?.[0]).toBe("/api/v1/runtimes");
    expect(fetch.mock.calls[0]?.[1]?.method || "GET").toBe("GET");
  });
  it("rejects an invalid catalog instead of projecting missing installations", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    for (const result of [
      {},
      { available: [], runtimes: [{ name: "codex", display_name: "Codex", available: "false" }] },
      {
        available: [],
        runtimes: [
          {
            name: "antigravity",
            display_name: "Antigravity",
            available: true,
            mode_availability: { acp: "false" },
          },
        ],
      },
    ]) {
      vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
        new Response(JSON.stringify({ ok: true, result }), {
          headers: { "content-type": "application/json" },
        }),
      );
      const response = await fetchRuntimes();
      expect(response.ok).toBe(false);
      expect(response.ok ? "" : response.error.code).toBe("invalid_response");
    }
  });
});

describe("filesystem API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("creates a child directory with a JSON request", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ ok: true, result: { path: "/projects/demo" } }), {
          headers: { "content-type": "application/json" },
        }),
      );

    const response = await createDirectory("/projects", "demo");

    expect(response).toEqual({ ok: true, result: { path: "/projects/demo" } });
    const [url, init] = fetchMock.mock.calls[0] || [];
    expect(url).toBe("/api/v1/fs/directory");
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toEqual({ parent: "/projects", name: "demo" });
  });

  it("rejects a malformed success response", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ ok: true, result: {} }), {
        headers: { "content-type": "application/json" },
      }),
    );

    const response = await createDirectory("/projects", "demo");

    expect(response.ok).toBe(false);
    expect(response.ok ? "" : response.error.code).toBe("invalid_response");
  });
});
