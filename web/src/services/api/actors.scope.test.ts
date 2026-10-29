// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { addActor, updateActor } from "./actors";
import { setAuthToken } from "./base";
afterEach(() => {
  vi.unstubAllGlobals();
  setAuthToken("");
});
it("keeps default-scope semantics and forwards an opaque selected key for custom and Profile Actors", async () => {
  setAuthToken("");
  const fetch = vi
    .fn()
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  vi.stubGlobal("fetch", fetch);
  await addActor("g_fixture", "default", "peer", "codex", "");
  await addActor("g_fixture", "fixed", "peer", "codex", "", undefined, {
    defaultScopeKey: "s_opaque",
  });
  await addActor("g_fixture", "profile", "peer", "codex", "", undefined, {
    profileId: "profile-1",
    defaultScopeKey: "s_other",
  });
  expect(fetch.mock.calls.map((call) => JSON.parse(call[1].body).default_scope_key)).toEqual([
    "",
    "s_opaque",
    "s_other",
  ]);
});

it("updates only the Actor directory, including explicit Follow Group, and otherwise omits it", async () => {
  const fetch = vi
    .fn()
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  vi.stubGlobal("fetch", fetch);
  await updateActor("g/fixture", "managed", undefined, undefined, undefined, {
    defaultScopeKey: "s_opaque",
  });
  await updateActor("g/fixture", "profile", undefined, undefined, undefined, {
    profileId: "profile-1",
    defaultScopeKey: "",
  });
  await updateActor("g/fixture", "unchanged", undefined, undefined, "New title");

  expect(fetch.mock.calls.map(([url, init]) => [url, init.method])).toEqual([
    ["/api/v1/groups/g%2Ffixture/actors/managed", "POST"],
    ["/api/v1/groups/g%2Ffixture/actors/profile", "POST"],
    ["/api/v1/groups/g%2Ffixture/actors/unchanged", "POST"],
  ]);
  const bodies = fetch.mock.calls.map(([, init]) => JSON.parse(init.body));
  expect(bodies[0]).toEqual({ by: "user", default_scope_key: "s_opaque" });
  expect(bodies[1]).toEqual({ by: "user", profile_id: "profile-1", default_scope_key: "" });
  expect(bodies[2]).not.toHaveProperty("default_scope_key");
});
