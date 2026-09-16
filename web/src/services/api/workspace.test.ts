import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  fetchWorkspaceListing,
  workspaceContentUrl,
  uploadWorkspaceFile,
  changeWorkspaceEntry,
  fetchWorkspaceChanges,
} from "./workspace";

describe("workspace API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("carries frame authority and exact workspace paths into native media and downloads", () => {
    const proof = btoa(
      JSON.stringify({
        frame_id: "media-frame",
        target_instance_id: "target",
        target_device_id: "device",
        parent_origin: "https://entry.example",
        expires_at: "2030-01-01T00:00:00Z",
        signature: "fixture",
      }),
    );
    const location = new URL(
      `https://target.example/ui/connect?proof=${encodeURIComponent(proof)}`,
    );
    vi.stubGlobal("window", { location });
    const file = {
      scope_key: "scope a",
      scope_url: "/project a",
      path: String.raw`图 #1\\clip.mp4`,
    };
    for (const download of [false, true]) {
      const url = new URL(workspaceContentUrl("g_media", file, download), location);
      expect(url.pathname).toBe("/api/v1/groups/g_media/workspace/content");
      expect(url.searchParams.get("path")).toBe(file.path);
      expect(url.searchParams.get("scope_key")).toBe(file.scope_key);
      expect(url.searchParams.get("scope_url")).toBe(file.scope_url);
      expect(url.searchParams.get("connect_frame")).toBe("media-frame");
      expect(url.searchParams.get("download")).toBe(download ? "true" : null);
      expect(url.searchParams.has("token")).toBe(false);
    }
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
  it("uses raw upload bytes, exact metadata and frame authority for all new workspace APIs", async () => {
    const proof = btoa(
      JSON.stringify({
        frame_id: "upload-frame",
        target_instance_id: "target",
        target_device_id: "device",
        parent_origin: "https://entry.example",
        expires_at: "2030-01-01T00:00:00Z",
        signature: "fixture",
      }),
    );
    const location = new URL(
      `https://target.example/ui/connect?proof=${encodeURIComponent(proof)}`,
    );
    vi.stubGlobal("window", { location });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation(
        async () =>
          new Response(JSON.stringify({ ok: true, result: {} }), {
            headers: { "content-type": "application/json" },
          }),
      );
    const file = new File(["hello"], "name.txt");
    const controller = new AbortController();
    await uploadWorkspaceFile("g", "scope", "/repo", "dir/name.txt", file, controller.signal);
    await changeWorkspaceEntry("g", "scope", "/repo", { operation: "delete", path: "name.txt" });
    await fetchWorkspaceChanges("g", "scope", "/repo", controller.signal);
    for (const [url] of fetchMock.mock.calls)
      expect(new URL(String(url), location).searchParams.get("connect_frame")).toBe("upload-frame");
    const [url, options] = fetchMock.mock.calls[0];
    expect(new URL(String(url), location).searchParams.get("bytes")).toBe("5");
    expect(options?.body).toBe(file);
    expect(new Headers(options?.headers).get("content-type")).toBe("application/octet-stream");
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

it("resolves path type with exact scope identity, including the root and literal filenames", async () => {
  const { resolveWorkspacePath } = await import("./workspace");
  const fetchMock = vi.spyOn(globalThis, "fetch");
  try {
    for (const path of ["", String.raw`literal\name #1.txt`, ".pytest_cache/v/cache"]) {
      fetchMock.mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              scope_key: "scope-a",
              scope_url: "/repo",
              path,
              is_dir: !path.endsWith(".txt"),
            },
          }),
          { headers: { "content-type": "application/json" } },
        ),
      );
      expect(await resolveWorkspacePath("g_paths", path, "scope-a", "/repo")).toEqual({
        ok: true,
        result: { path, is_dir: !path.endsWith(".txt") },
      });
      const url = new URL(String(fetchMock.mock.calls.at(-1)![0]), "http://localhost");
      expect(url.pathname).toBe("/api/v1/groups/g_paths/workspace/path");
      expect(url.searchParams.get("path")).toBe(path);
      expect(url.searchParams.get("scope_key")).toBe("scope-a");
      expect(url.searchParams.get("scope_url")).toBe("/repo");
    }
    fetchMock.mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: true,
          result: { scope_key: "old-scope", scope_url: "/repo", path: "", is_dir: true },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
    expect((await resolveWorkspacePath("g_paths", "", "scope-a", "/repo")).ok).toBe(false);
  } finally {
    vi.restoreAllMocks();
  }
});
