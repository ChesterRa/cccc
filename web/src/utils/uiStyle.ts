import { useSyncExternalStore } from "react";

/**
 * Interface style preview (T029). "classic" is the current look; "lcd" is the
 * LCD pixel style being evaluated, applied only inside elements marked
 * `data-lcd-scope`. Stored per browser; the body-font knob exists only while
 * the user compares fonts on real pages.
 */
export type UiStyle = "classic" | "lcd";
export type LcdBodyFont = "mono" | "pixel";
export type UiStylePrefs = { style: UiStyle; lcdBody: LcdBodyFont };

export const UI_STYLE_STORAGE_KEY = "cccc-ui-style";
export const LCD_BODY_STORAGE_KEY = "cccc-ui-lcd-body";

export const DEFAULT_UI_STYLE_PREFS: UiStylePrefs = { style: "classic", lcdBody: "mono" };

export function normalizeUiStylePrefs(raw: { style?: unknown; lcdBody?: unknown }): UiStylePrefs {
  return {
    style: raw.style === "lcd" ? "lcd" : "classic",
    lcdBody: raw.lcdBody === "pixel" ? "pixel" : "mono",
  };
}

function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Blocked storage keeps the choice for this page only.
  }
}

export function readStoredUiStylePrefs(): UiStylePrefs {
  if (typeof window === "undefined") return DEFAULT_UI_STYLE_PREFS;
  return normalizeUiStylePrefs({
    style: readStorage(UI_STYLE_STORAGE_KEY),
    lcdBody: readStorage(LCD_BODY_STORAGE_KEY),
  });
}

export function applyUiStylePrefs(prefs: UiStylePrefs) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (prefs.style === "lcd") {
    root.dataset.uiStyle = "lcd";
    root.dataset.lcdBody = prefs.lcdBody;
  } else {
    delete root.dataset.uiStyle;
    delete root.dataset.lcdBody;
  }
}

let current: UiStylePrefs = readStoredUiStylePrefs();
const listeners = new Set<() => void>();

export function getUiStylePrefs(): UiStylePrefs {
  return current;
}

export function setUiStylePrefs(patch: Partial<UiStylePrefs>) {
  const next = normalizeUiStylePrefs({ ...current, ...patch });
  if (next.style === current.style && next.lcdBody === current.lcdBody) return;
  current = next;
  writeStorage(UI_STYLE_STORAGE_KEY, next.style);
  writeStorage(LCD_BODY_STORAGE_KEY, next.lcdBody);
  applyUiStylePrefs(next);
  listeners.forEach((listener) => listener());
}

export function subscribeUiStylePrefs(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useUiStylePrefs(): UiStylePrefs {
  return useSyncExternalStore(subscribeUiStylePrefs, getUiStylePrefs, () => DEFAULT_UI_STYLE_PREFS);
}

/** True while the LCD preview is on; layout containers branch on this. */
export function useLcdStyle(): boolean {
  return useUiStylePrefs().style === "lcd";
}
