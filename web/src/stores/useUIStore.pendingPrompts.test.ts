// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vite-plus/test";

beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

it("shows sidebar waiting badges by default and remembers the choice after reload", async () => {
  const { useUIStore } = await import("./useUIStore");
  expect(useUIStore.getState().showPendingPrompts).toBe(true);
  useUIStore.getState().setShowPendingPrompts(false);
  vi.resetModules();
  const reloaded = await import("./useUIStore");
  expect(reloaded.useUIStore.getState().showPendingPrompts).toBe(false);
  reloaded.useUIStore.getState().setShowPendingPrompts(true);
  vi.resetModules();
  expect((await import("./useUIStore")).useUIStore.getState().showPendingPrompts).toBe(true);
});

it("defaults to showing badges for an unrecognized stored value", async () => {
  localStorage.setItem("cccc-pending-prompts", "unexpected");
  const { useUIStore } = await import("./useUIStore");
  expect(useUIStore.getState().showPendingPrompts).toBe(true);
});

it("keeps a choice for the current page when storage is unavailable", async () => {
  const blocked = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
  };
  const storage = vi.spyOn(window, "localStorage", "get").mockReturnValue(blocked as Storage);
  try {
    const { useUIStore } = await import("./useUIStore");
    expect(useUIStore.getState().showPendingPrompts).toBe(true);
    useUIStore.getState().setShowPendingPrompts(false);
    expect(useUIStore.getState().showPendingPrompts).toBe(false);
  } finally {
    storage.mockRestore();
  }
});
