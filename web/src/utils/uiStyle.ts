import { useSyncExternalStore } from "react";

/**
 * Appearance choices of the LCD look, the Web UI's only style, stored per browser:
 * the body font, the width of the conversation on desktop (a centred reading column,
 * wide, or the full width) and motion. index.html marks the page as LCD before the
 * first paint; these choices are applied as data attributes on the root element.
 */
export type LcdBodyFont = "mono" | "pixel";
export type LcdWidth = "column" | "wide" | "full";
export type LcdMotion = "on" | "off";
export type UiStylePrefs = { lcdBody: LcdBodyFont; lcdWidth: LcdWidth; lcdMotion: LcdMotion };

export const LCD_BODY_STORAGE_KEY = "cccc-ui-lcd-body";
export const LCD_WIDTH_STORAGE_KEY = "cccc-ui-lcd-width";
export const LCD_MOTION_STORAGE_KEY = "cccc-ui-lcd-motion";

const STORAGE_KEYS: Record<keyof UiStylePrefs, string> = {
  lcdBody: LCD_BODY_STORAGE_KEY,
  lcdWidth: LCD_WIDTH_STORAGE_KEY,
  lcdMotion: LCD_MOTION_STORAGE_KEY,
};

export const DEFAULT_UI_STYLE_PREFS: UiStylePrefs = {
  lcdBody: "mono",
  lcdWidth: "column",
  lcdMotion: "on",
};

export function normalizeUiStylePrefs(raw: {
  lcdBody?: unknown;
  lcdWidth?: unknown;
  lcdMotion?: unknown;
}): UiStylePrefs {
  return {
    lcdBody: raw.lcdBody === "pixel" ? "pixel" : "mono",
    lcdWidth: raw.lcdWidth === "wide" || raw.lcdWidth === "full" ? raw.lcdWidth : "column",
    lcdMotion: raw.lcdMotion === "off" ? "off" : "on",
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
    lcdBody: readStorage(LCD_BODY_STORAGE_KEY),
    lcdWidth: readStorage(LCD_WIDTH_STORAGE_KEY),
    lcdMotion: readStorage(LCD_MOTION_STORAGE_KEY),
  });
}

function reducedMotionQuery(): MediaQueryList | null {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return null;
  return window.matchMedia("(prefers-reduced-motion: reduce)");
}

/**
 * `data-lcd-motion` is the effective setting: off when the user turns motion off or
 * the system asks for reduced motion, so each still form needs one rule.
 */
export function applyUiStylePrefs(prefs: UiStylePrefs) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.dataset.lcdBody = prefs.lcdBody;
  root.dataset.lcdWidth = prefs.lcdWidth;
  root.dataset.lcdMotion =
    prefs.lcdMotion === "off" || reducedMotionQuery()?.matches ? "off" : "on";
}

let current: UiStylePrefs = readStoredUiStylePrefs();
const listeners = new Set<() => void>();
reducedMotionQuery()?.addEventListener?.("change", () => applyUiStylePrefs(current));

export function getUiStylePrefs(): UiStylePrefs {
  return current;
}

export function setUiStylePrefs(patch: Partial<UiStylePrefs>) {
  const next = normalizeUiStylePrefs({ ...current, ...patch });
  const changed = (Object.keys(STORAGE_KEYS) as (keyof UiStylePrefs)[]).filter(
    (field) => next[field] !== current[field],
  );
  if (!changed.length) return;
  current = next;
  // Write only what changed: another open tab may have stored newer values for the rest.
  for (const field of changed) writeStorage(STORAGE_KEYS[field], next[field]);
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

function subscribeReducedMotion(listener: () => void) {
  const query = reducedMotionQuery();
  query?.addEventListener?.("change", listener);
  return () => query?.removeEventListener?.("change", listener);
}

/** True while the system asks for reduced motion (it stills LCD motion whatever is chosen). */
export function useSystemReducedMotion(): boolean {
  return useSyncExternalStore(
    subscribeReducedMotion,
    () => reducedMotionQuery()?.matches ?? false,
    () => false,
  );
}
