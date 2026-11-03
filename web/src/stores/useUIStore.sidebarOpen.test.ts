// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";

afterEach(() => {
  window.history.replaceState(null, "", "/");
  vi.resetModules();
});

/** The phone's Group list drawer as the page loads at this address. */
async function sidebarOpenAt(path: string) {
  window.history.replaceState(null, "", path);
  vi.resetModules();
  const { useUIStore } = await import("./useUIStore");
  return useUIStore.getState().sidebarOpen;
}

it("opens the Group list on a visit without a Group", async () => {
  expect(await sidebarOpenAt("/ui/")).toBe(true);
  expect(await sidebarOpenAt("/ui/?group=%20")).toBe(true);
});

it("shows the conversation, not the Group list, on a Group's or a message's address", async () => {
  // A reload keeps ?group=; a notification opens ?group=&event=.
  expect(await sidebarOpenAt("/ui/?group=g_1")).toBe(false);
  expect(await sidebarOpenAt("/ui/?group=g_1&event=ev_1")).toBe(false);
});
