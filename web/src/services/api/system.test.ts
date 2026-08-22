import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { createDirectory } from "./system";

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
