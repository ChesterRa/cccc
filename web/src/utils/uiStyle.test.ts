// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  DEFAULT_UI_STYLE_PREFS,
  LCD_BODY_STORAGE_KEY,
  UI_STYLE_STORAGE_KEY,
  normalizeUiStylePrefs,
} from "./uiStyle";

// The store reads storage once at import, so each case loads a fresh module.
async function loadStore() {
  vi.resetModules();
  return import("./uiStyle");
}

beforeEach(() => {
  window.localStorage.clear();
  const root = document.documentElement;
  delete root.dataset.uiStyle;
  delete root.dataset.lcdBody;
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("interface style preference", () => {
  it("accepts only known values and falls back to the classic defaults", () => {
    expect(normalizeUiStylePrefs({ style: "lcd", lcdBody: "pixel" })).toEqual({
      style: "lcd",
      lcdBody: "pixel",
    });
    expect(normalizeUiStylePrefs({ style: "LCD", lcdBody: 1 })).toEqual(DEFAULT_UI_STYLE_PREFS);
  });

  it("restores the stored choice and ignores invalid stored values", async () => {
    window.localStorage.setItem(UI_STYLE_STORAGE_KEY, "lcd");
    window.localStorage.setItem(LCD_BODY_STORAGE_KEY, "serif");
    const store = await loadStore();
    expect(store.getUiStylePrefs()).toEqual({ style: "lcd", lcdBody: "mono" });
  });

  it("starts classic when storage cannot be read", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const store = await loadStore();
    expect(store.getUiStylePrefs()).toEqual(DEFAULT_UI_STYLE_PREFS);
  });

  it("applies, stores and announces a change, and clears the page marks for classic", async () => {
    const store = await loadStore();
    const listener = vi.fn();
    const root = document.documentElement;
    const unsubscribe = store.subscribeUiStylePrefs(listener);

    store.setUiStylePrefs({ style: "lcd", lcdBody: "pixel" });
    expect(root.dataset.uiStyle).toBe("lcd");
    expect(root.dataset.lcdBody).toBe("pixel");
    expect(window.localStorage.getItem(UI_STYLE_STORAGE_KEY)).toBe("lcd");
    expect(window.localStorage.getItem(LCD_BODY_STORAGE_KEY)).toBe("pixel");
    expect(listener).toHaveBeenCalledTimes(1);

    store.setUiStylePrefs({ style: "lcd" });
    expect(listener).toHaveBeenCalledTimes(1);

    store.setUiStylePrefs({ style: "classic" });
    expect(root.dataset.uiStyle).toBeUndefined();
    expect(root.dataset.lcdBody).toBeUndefined();
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  it("keeps a change for this page when storage cannot be written", async () => {
    const store = await loadStore();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    store.setUiStylePrefs({ style: "lcd" });
    expect(store.getUiStylePrefs().style).toBe("lcd");
    expect(document.documentElement.dataset.uiStyle).toBe("lcd");
  });
});
