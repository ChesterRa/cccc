import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

function makeStorage() {
  const data = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, String(value));
    }),
    removeItem: vi.fn((key: string) => {
      data.delete(key);
    }),
    clear: vi.fn(() => {
      data.clear();
    }),
  };
}

describe("composer message mode preference", () => {
  let storage: ReturnType<typeof makeStorage>;

  beforeEach(() => {
    vi.resetModules();
    storage = makeStorage();
    vi.stubGlobal("localStorage", storage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("defaults a fresh browser to Need Reply", async () => {
    const mod = await import("../../src/stores/useComposerStore");

    expect(mod.loadComposerMessageModePreference()).toBe("reply");
    expect(mod.useComposerStore.getState()).toMatchObject({
      preferredMessageMode: "reply",
      priority: "normal",
      replyRequired: true,
    });
  });

  it("restores the last valid browser preference and rejects malformed values", async () => {
    storage.setItem("cccc-composer-message-mode", "attention");
    let mod = await import("../../src/stores/useComposerStore");
    expect(mod.useComposerStore.getState()).toMatchObject({
      preferredMessageMode: "attention",
      priority: "attention",
      replyRequired: false,
    });

    storage.setItem(mod.COMPOSER_MESSAGE_MODE_STORAGE_KEY, "unknown");
    vi.resetModules();
    mod = await import("../../src/stores/useComposerStore");
    expect(mod.useComposerStore.getState()).toMatchObject({
      preferredMessageMode: "reply",
      priority: "normal",
      replyRequired: true,
    });
  });

  it("persists an explicit choice and keeps it after clearing a sent composer", async () => {
    const mod = await import("../../src/stores/useComposerStore");
    const store = mod.useComposerStore;

    store.getState().setMessageMode("normal");
    store.getState().setComposerText("status only");
    store.getState().clearComposer();

    expect(storage.getItem(mod.COMPOSER_MESSAGE_MODE_STORAGE_KEY)).toBe("normal");
    expect(store.getState()).toMatchObject({
      preferredMessageMode: "normal",
      priority: "normal",
      replyRequired: false,
      composerText: "",
    });
  });

  it("restores a draft's own mode without replacing the newer browser preference", async () => {
    const mod = await import("../../src/stores/useComposerStore");
    const store = mod.useComposerStore;

    store.getState().switchGroup(null, "g-a");
    store.getState().setComposerText("reply-required draft");
    store.getState().switchGroup("g-a", "g-b");
    store.getState().setMessageMode("normal");

    store.getState().switchGroup("g-b", "g-a");
    expect(store.getState()).toMatchObject({
      composerText: "reply-required draft",
      preferredMessageMode: "normal",
      priority: "normal",
      replyRequired: true,
    });

    store.getState().clearComposer();
    expect(store.getState()).toMatchObject({
      preferredMessageMode: "normal",
      priority: "normal",
      replyRequired: false,
    });
  });

  it("falls back safely when browser storage is unavailable", async () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("storage unavailable");
      },
      setItem: () => {
        throw new Error("storage unavailable");
      },
    });

    const mod = await import("../../src/stores/useComposerStore");
    expect(mod.useComposerStore.getState().preferredMessageMode).toBe("reply");
    expect(() => mod.useComposerStore.getState().setMessageMode("attention")).not.toThrow();
    expect(mod.useComposerStore.getState()).toMatchObject({
      preferredMessageMode: "attention",
      priority: "attention",
      replyRequired: false,
    });
    expect(warning).toHaveBeenCalledTimes(2);
  });
});
