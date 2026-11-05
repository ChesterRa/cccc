// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { fetchActors } from "./actors";
import { getGroupBlobUrl } from "./groups";
import { setAuthToken } from "./base";

afterEach(() => {
  vi.unstubAllGlobals();
  setAuthToken("");
});

it("projects persisted avatar blobs into actor images and drops them after reset", async () => {
  const actor = { id: "native", avatar_asset_path: "state/blobs/0123456789abcdef" };
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, result: { actors: [actor] } })))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ ok: true, result: { actors: [{ ...actor, avatar_asset_path: "" }] } }),
      ),
    );
  vi.stubGlobal("fetch", fetch);

  const before = await fetchActors("g fixture", false, { noCache: true });
  expect(before.ok).toBe(true);
  if (!before.ok) return;
  const image = before.result.actors[0];
  expect(image.has_custom_avatar).toBe(true);
  expect(image.avatar_url).toBe("/api/v1/groups/g%20fixture/blobs/0123456789abcdef");
  // Chat sender snapshots use the same content-addressed URL and survive a
  // later avatar change or removal.
  expect(getGroupBlobUrl("g fixture", actor.avatar_asset_path)).toBe(image.avatar_url);

  const after = await fetchActors("g fixture", false, { noCache: true });
  expect(after.ok).toBe(true);
  if (!after.ok) return;
  expect(after.result.actors[0].avatar_url).toBeUndefined();
  expect(after.result.actors[0].has_custom_avatar).toBeUndefined();
});
