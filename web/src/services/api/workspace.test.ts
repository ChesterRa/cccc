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
        new Response(
          JSON.stringify({
            ok: true,
            result: { scope_key: "scope-a", scope_url: "/repo", path: "", parent: null, items: [] },
          }),
          { headers: { "content-type": "application/json" } },
        ),
      );

    await fetchWorkspaceListing("group-1", "", {
      showIgnored: true,
      scopeKey: "scope-a",
      scopeUrl: "/repo",
    });

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
        new Response(
          JSON.stringify({
            ok: true,
            result: { scope_key: "scope-a", scope_url: "/repo", path: "", parent: null, items: [] },
          }),
          { headers: { "content-type": "application/json" } },
        ),
      );

    await fetchWorkspaceListing("group-1", "src", { scopeKey: "scope-a", scopeUrl: "/repo" });

    const [url] = fetchMock.mock.calls[0] || [];
    expect(String(url)).toContain("path=src");
    expect(String(url)).not.toContain("show_ignored");
  });
});

it("round-trips literal paths and the opened workspace identity through read and save", async () => {
  const { fetchWorkspaceFile, saveWorkspaceFile } = await import("./workspace");
  const scope = { scope_key: "scope-a", scope_url: "/project a" };
  const path = String.raw`foo\bar.txt`;
  const fetchMock = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: true,
          result: { ...scope, path, content: "original", sha256: "old" },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
  try {
    const opened = await fetchWorkspaceFile("g", path, scope.scope_key, scope.scope_url);
    expect(opened.ok).toBe(true);
    const query = new URL(String(fetchMock.mock.calls[0][0]), "http://localhost").searchParams;
    expect(query.get("path")).toBe(path);
    expect(query.get("scope_url")).toBe(scope.scope_url);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ ok: true, result: { path, sha256: "new", created: false } }), {
        headers: { "content-type": "application/json" },
      }),
    );
    await saveWorkspaceFile("g", path, "edited", "old", scope.scope_key, scope.scope_url);
    expect(JSON.parse(fetchMock.mock.calls.at(-1)![1]!.body as string)).toEqual({
      ...scope,
      path,
      content: "edited",
      sha256: "old",
    });
  } finally {
    vi.restoreAllMocks();
  }
});
