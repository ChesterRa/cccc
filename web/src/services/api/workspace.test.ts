import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { fetchWorkspaceListing } from "./workspace";

describe("workspace API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("sends a boolean the Axum query deserializer accepts", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ ok: true, result: { path: "", parent: null, items: [] } }), {
          headers: { "content-type": "application/json" },
        }),
      );

    await fetchWorkspaceListing("group-1", "", { showIgnored: true });

    const [url] = fetchMock.mock.calls[0] || [];
    // `show_ignored=1` is rejected by serde with "provided string was not `true` or `false`",
    // which turned the whole tree into an error instead of revealing ignored files.
    expect(String(url)).toContain("show_ignored=true");
    expect(String(url)).not.toContain("show_ignored=1");
  });

  it("omits the flag entirely when ignored files stay hidden", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ ok: true, result: { path: "", parent: null, items: [] } }), {
          headers: { "content-type": "application/json" },
        }),
      );

    await fetchWorkspaceListing("group-1", "src");

    const [url] = fetchMock.mock.calls[0] || [];
    expect(String(url)).toContain("path=src");
    expect(String(url)).not.toContain("show_ignored");
  });
});
