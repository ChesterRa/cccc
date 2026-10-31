// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  DEFAULT_UI_STYLE_PREFS,
  LCD_BODY_STORAGE_KEY,
  LCD_MOTION_STORAGE_KEY,
  LCD_WIDTH_STORAGE_KEY,
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
  delete root.dataset.lcdWidth;
  delete root.dataset.lcdMotion;
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("interface style preference", () => {
  it("accepts only known values and falls back to the classic defaults", () => {
    expect(
      normalizeUiStylePrefs({ style: "lcd", lcdBody: "pixel", lcdWidth: "full", lcdMotion: "off" }),
    ).toEqual({ style: "lcd", lcdBody: "pixel", lcdWidth: "full", lcdMotion: "off" });
    expect(normalizeUiStylePrefs({ style: "lcd", lcdWidth: "wide" }).lcdWidth).toBe("wide");
    expect(
      normalizeUiStylePrefs({ style: "LCD", lcdBody: 1, lcdWidth: "max", lcdMotion: false }),
    ).toEqual(DEFAULT_UI_STYLE_PREFS);
  });

  it("restores the stored choice and ignores invalid stored values", async () => {
    window.localStorage.setItem(UI_STYLE_STORAGE_KEY, "lcd");
    window.localStorage.setItem(LCD_BODY_STORAGE_KEY, "serif");
    window.localStorage.setItem(LCD_WIDTH_STORAGE_KEY, "full");
    window.localStorage.setItem(LCD_MOTION_STORAGE_KEY, "off");
    const store = await loadStore();
    expect(store.getUiStylePrefs()).toEqual({
      style: "lcd",
      lcdBody: "mono",
      lcdWidth: "full",
      lcdMotion: "off",
    });
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

    expect(root.dataset.lcdWidth).toBe("column");

    store.setUiStylePrefs({ style: "lcd" });
    expect(listener).toHaveBeenCalledTimes(1);

    store.setUiStylePrefs({ lcdWidth: "full" });
    expect(root.dataset.lcdWidth).toBe("full");
    expect(window.localStorage.getItem(LCD_WIDTH_STORAGE_KEY)).toBe("full");
    expect(listener).toHaveBeenCalledTimes(2);

    store.setUiStylePrefs({ style: "classic" });
    expect(root.dataset.uiStyle).toBeUndefined();
    expect(root.dataset.lcdBody).toBeUndefined();
    expect(root.dataset.lcdWidth).toBeUndefined();
    expect(store.getUiStylePrefs().lcdWidth).toBe("full");
    expect(listener).toHaveBeenCalledTimes(3);
    unsubscribe();
  });

  it("marks motion off for the user's choice or the system's reduced-motion setting", async () => {
    let reduce = false;
    let notify: (() => void) | undefined;
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          media: query,
          get matches() {
            return reduce;
          },
          addEventListener: (_type: string, listener: () => void) => {
            notify = listener;
          },
          removeEventListener: () => {},
        }) as unknown as MediaQueryList,
    );
    const store = await loadStore();
    const root = document.documentElement;

    store.setUiStylePrefs({ style: "lcd" });
    expect(root.dataset.lcdMotion).toBe("on");
    store.setUiStylePrefs({ lcdMotion: "off" });
    expect(root.dataset.lcdMotion).toBe("off");
    expect(window.localStorage.getItem(LCD_MOTION_STORAGE_KEY)).toBe("off");
    store.setUiStylePrefs({ lcdMotion: "on" });
    expect(root.dataset.lcdMotion).toBe("on");

    reduce = true;
    notify?.();
    expect(root.dataset.lcdMotion).toBe("off");
    expect(store.getUiStylePrefs().lcdMotion).toBe("on");

    store.setUiStylePrefs({ style: "classic" });
    expect(root.dataset.lcdMotion).toBeUndefined();
  });

  it("stores only the changed choice, keeping what another tab stored for the rest", async () => {
    const store = await loadStore();
    store.setUiStylePrefs({ style: "lcd" });
    window.localStorage.setItem(LCD_WIDTH_STORAGE_KEY, "full");
    store.setUiStylePrefs({ lcdBody: "pixel" });
    expect(window.localStorage.getItem(LCD_BODY_STORAGE_KEY)).toBe("pixel");
    expect(window.localStorage.getItem(LCD_WIDTH_STORAGE_KEY)).toBe("full");
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
