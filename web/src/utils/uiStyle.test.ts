// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  DEFAULT_UI_STYLE_PREFS,
  LCD_BODY_STORAGE_KEY,
  LCD_MOTION_STORAGE_KEY,
  LCD_WIDTH_STORAGE_KEY,
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

describe("LCD appearance preferences", () => {
  it("accepts only known values and falls back to the defaults", () => {
    expect(normalizeUiStylePrefs({ lcdBody: "pixel", lcdWidth: "full", lcdMotion: "off" })).toEqual(
      { style: "classic", lcdBody: "pixel", lcdWidth: "full", lcdMotion: "off" },
    );
    expect(normalizeUiStylePrefs({ lcdWidth: "wide" }).lcdWidth).toBe("wide");
    expect(normalizeUiStylePrefs({ lcdBody: 1, lcdWidth: "max", lcdMotion: false })).toEqual(
      DEFAULT_UI_STYLE_PREFS,
    );
  });

  it("restores the stored choices and ignores invalid stored values", async () => {
    window.localStorage.setItem(LCD_BODY_STORAGE_KEY, "serif");
    window.localStorage.setItem(LCD_WIDTH_STORAGE_KEY, "full");
    window.localStorage.setItem(LCD_MOTION_STORAGE_KEY, "off");
    const store = await loadStore();
    expect(store.getUiStylePrefs()).toEqual({
      style: "classic",
      lcdBody: "mono",
      lcdWidth: "full",
      lcdMotion: "off",
    });
  });

  it("starts from the defaults when storage cannot be read", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const store = await loadStore();
    expect(store.getUiStylePrefs()).toEqual(DEFAULT_UI_STYLE_PREFS);
  });

  it("applies, stores and announces a change", async () => {
    const store = await loadStore();
    const listener = vi.fn();
    const root = document.documentElement;
    const unsubscribe = store.subscribeUiStylePrefs(listener);

    store.setUiStylePrefs({ lcdBody: "pixel" });
    expect(root.dataset.lcdBody).toBe("pixel");
    expect(root.dataset.lcdWidth).toBe("column");
    expect(window.localStorage.getItem(LCD_BODY_STORAGE_KEY)).toBe("pixel");
    expect(listener).toHaveBeenCalledTimes(1);

    store.setUiStylePrefs({ lcdBody: "pixel" });
    expect(listener).toHaveBeenCalledTimes(1);

    store.setUiStylePrefs({ lcdWidth: "full" });
    expect(root.dataset.lcdWidth).toBe("full");
    expect(window.localStorage.getItem(LCD_WIDTH_STORAGE_KEY)).toBe("full");
    expect(listener).toHaveBeenCalledTimes(2);
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

    store.applyUiStylePrefs(store.getUiStylePrefs());
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
  });

  it("stores only the changed choice, keeping what another tab stored for the rest", async () => {
    const store = await loadStore();
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
    store.setUiStylePrefs({ lcdBody: "pixel" });
    expect(store.getUiStylePrefs().lcdBody).toBe("pixel");
    expect(document.documentElement.dataset.lcdBody).toBe("pixel");
  });
});
