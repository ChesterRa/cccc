export type ResolvedTheme = "light" | "dark";

export const LIGHT_THEME_START_HOUR = 7;
export const DARK_THEME_START_HOUR = 19;

/** Resolve the automatic theme from the browser's local time zone. */
export function getAutomaticTheme(now = new Date()): ResolvedTheme {
  const hour = now.getHours();
  return hour >= LIGHT_THEME_START_HOUR && hour < DARK_THEME_START_HOUR ? "light" : "dark";
}

export function millisecondsUntilNextThemeBoundary(now = new Date()): number {
  const nextBoundary = new Date(now);
  const hour = now.getHours();

  if (hour < LIGHT_THEME_START_HOUR) {
    nextBoundary.setHours(LIGHT_THEME_START_HOUR, 0, 0, 0);
  } else if (hour < DARK_THEME_START_HOUR) {
    nextBoundary.setHours(DARK_THEME_START_HOUR, 0, 0, 0);
  } else {
    nextBoundary.setDate(nextBoundary.getDate() + 1);
    nextBoundary.setHours(LIGHT_THEME_START_HOUR, 0, 0, 0);
  }

  return Math.max(1_000, nextBoundary.getTime() - now.getTime());
}

/** Keep automatic mode aligned with local-time boundaries and time-zone changes. */
export function subscribeToAutomaticTheme(callback: () => void): () => void {
  let boundaryTimer: number | undefined;

  const scheduleBoundary = () => {
    if (boundaryTimer !== undefined) window.clearTimeout(boundaryTimer);
    boundaryTimer = window.setTimeout(() => {
      callback();
      scheduleBoundary();
    }, millisecondsUntilNextThemeBoundary() + 50);
  };
  const refresh = () => {
    callback();
    scheduleBoundary();
  };

  scheduleBoundary();
  window.addEventListener("focus", refresh);
  document.addEventListener("visibilitychange", refresh);

  return () => {
    if (boundaryTimer !== undefined) window.clearTimeout(boundaryTimer);
    window.removeEventListener("focus", refresh);
    document.removeEventListener("visibilitychange", refresh);
  };
}
