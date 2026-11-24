import { useSyncExternalStore } from "react";

/**
 * Appearance choices of the LCD look, the Web UI's only style, stored per browser:
 * the body font, the width of the conversation on desktop (a centred reading column,
 * wide, or the full width), motion, the style (square or soft corners, lcdSoft.css) and the
 * skin: a palette laid over the LCD look (`styles/skins/`), or none for the plain one. The
 * style and the skin are chosen independently. index.html marks the page as LCD before the
 * first paint; these choices are applied as data attributes on the root element.
 */
export type LcdBodyFont = "proportional" | "mono" | "pixel";
export type LcdWidth = "column" | "wide" | "full";
export type LcdMotion = "on" | "off";
/** The shape half of the appearance: square LCD corners or the soft style (lcdSoft.css). */
export type UiLook = "square" | "soft";
/** The colour half: the plain LCD palette, or a skin from `styles/skins/`. */
export type UiSkin = "classic" | "studio";
export const UI_SKINS: readonly UiSkin[] = ["classic", "studio"];
export type UiStylePrefs = {
  lcdBody: LcdBodyFont;
  lcdWidth: LcdWidth;
  lcdMotion: LcdMotion;
  skin: UiSkin;
  look: UiLook;
};

export const LCD_BODY_STORAGE_KEY = "cccc-ui-lcd-body";
export const LCD_WIDTH_STORAGE_KEY = "cccc-ui-lcd-width";
export const LCD_MOTION_STORAGE_KEY = "cccc-ui-lcd-motion";
/** index.html reads this key too, to paint the skin from the first frame. */
export const SKIN_STORAGE_KEY = "cccc-ui-skin";
/** index.html reads this key too, to paint the style from the first frame. */
export const LOOK_STORAGE_KEY = "cccc-ui-look";

const STORAGE_KEYS: Record<keyof UiStylePrefs, string> = {
  lcdBody: LCD_BODY_STORAGE_KEY,
  lcdWidth: LCD_WIDTH_STORAGE_KEY,
  lcdMotion: LCD_MOTION_STORAGE_KEY,
  skin: SKIN_STORAGE_KEY,
  look: LOOK_STORAGE_KEY,
};

export const DEFAULT_UI_STYLE_PREFS: UiStylePrefs = {
  lcdBody: "proportional",
  lcdWidth: "column",
  lcdMotion: "on",
  skin: "classic",
  look: "square",
};

export function normalizeUiStylePrefs(raw: {
  lcdBody?: unknown;
  lcdWidth?: unknown;
  lcdMotion?: unknown;
  skin?: unknown;
  look?: unknown;
}): UiStylePrefs {
  return {
    lcdBody: raw.lcdBody === "pixel" || raw.lcdBody === "mono" ? raw.lcdBody : "proportional",
    lcdWidth: raw.lcdWidth === "wide" || raw.lcdWidth === "full" ? raw.lcdWidth : "column",
    lcdMotion: raw.lcdMotion === "off" ? "off" : "on",
    skin: UI_SKINS.includes(raw.skin as UiSkin) ? (raw.skin as UiSkin) : "classic",
    look: raw.look === "soft" ? "soft" : "square",
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
    skin: readStorage(SKIN_STORAGE_KEY),
    look: readStorage(LOOK_STORAGE_KEY),
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
  // The plain palette carries no attribute, so no skin rule can match it.
  if (prefs.skin === "classic") delete root.dataset.skin;
  else root.dataset.skin = prefs.skin;
  root.dataset.look = prefs.look;
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
