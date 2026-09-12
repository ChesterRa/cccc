import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { COMPOSER_HEIGHT_KEY } from "../../src/utils/composerHeight";

const data = new Map<string, string>();
const storage = {
  getItem: vi.fn((key: string) => data.get(key) ?? null),
  setItem: vi.fn((key: string, value: string) => {
    data.set(key, value);
  }),
  removeItem: vi.fn((key: string) => {
    data.delete(key);
  }),
};
beforeEach(() => {
  vi.resetModules();
  data.clear();
  vi.clearAllMocks();
  vi.stubGlobal("localStorage", storage);
});
afterEach(() => vi.unstubAllGlobals());

describe("composer height preference store", () => {
  it("persists a clamped preference and restores it on reload", async () => {
    const { useUIStore } = await import("../../src/stores/useUIStore");
    useUIStore.getState().setComposerHeight(323.8);
    expect(useUIStore.getState().composerHeight).toBe(324);
    expect(storage.setItem).toHaveBeenCalledWith(COMPOSER_HEIGHT_KEY, "324");
    vi.resetModules();
    const reloaded = await import("../../src/stores/useUIStore");
    expect(reloaded.useUIStore.getState().composerHeight).toBe(324);
    reloaded.useUIStore.getState().setComposerHeight(-1);
    expect(reloaded.useUIStore.getState().composerHeight).toBe(64);
    reloaded.useUIStore.getState().setComposerHeight(null);
    expect(storage.removeItem).toHaveBeenCalledWith(COMPOSER_HEIGHT_KEY);
    vi.resetModules();
    const automatic = await import("../../src/stores/useUIStore");
    expect(automatic.useUIStore.getState().composerHeight).toBeNull();
  });
  it("keeps the in-memory preference usable when storage writes fail", async () => {
    const { useUIStore } = await import("../../src/stores/useUIStore");
    storage.setItem.mockImplementationOnce(() => {
      throw Error("blocked");
    });
    expect(() => useUIStore.getState().setComposerHeight(240)).not.toThrow();
    expect(useUIStore.getState().composerHeight).toBe(240);
  });
  it("uses automatic height when reading this preference fails", async () => {
    storage.getItem.mockImplementation((key) => {
      if (key === COMPOSER_HEIGHT_KEY) throw Error("blocked");
      return null;
    });
    const { useUIStore } = await import("../../src/stores/useUIStore");
    expect(useUIStore.getState().composerHeight).toBeNull();
    storage.getItem.mockImplementation((key) => data.get(key) ?? null);
  });
});
