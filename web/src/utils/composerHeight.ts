// Preferences use pixels at 100% text scale; the view converts to rendered pixels.
export const COMPOSER_DEFAULT_HEIGHT = 64;
export const COMPOSER_AUTO_MAX_HEIGHT = 128;
export const COMPOSER_HEIGHT_KEY = "cccc-composer-height";

export function clampComposerHeight(value: number, maximum = Number.MAX_SAFE_INTEGER): number {
  const upper = Number.isFinite(maximum)
    ? Math.max(COMPOSER_DEFAULT_HEIGHT, Math.floor(maximum))
    : COMPOSER_DEFAULT_HEIGHT;
  const numeric = Number.isFinite(value) ? Math.round(value) : COMPOSER_DEFAULT_HEIGHT;
  return Math.max(COMPOSER_DEFAULT_HEIGHT, Math.min(upper, numeric));
}

export function composerHeightLimit(
  panelHeight: number,
  chromeHeight: number,
  scale: number,
): number {
  const fontScale = Number.isFinite(scale) && scale > 0 ? scale : 1;
  // Leave room for the message list as well as recipient/action/status rows.
  return clampComposerHeight(
    Math.floor(
      Math.min(panelHeight * 0.6, panelHeight - chromeHeight - 80 * fontScale) / fontScale,
    ),
  );
}

export function loadComposerHeight(): number | null {
  try {
    const stored = localStorage.getItem(COMPOSER_HEIGHT_KEY);
    if (!stored?.trim() || !Number.isFinite(Number(stored))) return null;
    return clampComposerHeight(Number(stored));
  } catch {
    return null;
  }
}

export function saveComposerHeight(height: number | null): void {
  try {
    if (height === null) localStorage.removeItem(COMPOSER_HEIGHT_KEY);
    else localStorage.setItem(COMPOSER_HEIGHT_KEY, String(clampComposerHeight(height)));
  } catch {
    // Storage can be unavailable; the in-memory preference remains usable.
  }
}
