export type ResolvedTheme = "light" | "dark";

const DARK_SCHEME = "(prefers-color-scheme: dark)";

/** Resolve the automatic theme from the system's appearance setting. */
export function getAutomaticTheme(): ResolvedTheme {
  return window.matchMedia?.(DARK_SCHEME).matches ? "dark" : "light";
}

/** Keep automatic mode aligned with the system's appearance setting. */
export function subscribeToAutomaticTheme(callback: () => void): () => void {
  const scheme = window.matchMedia?.(DARK_SCHEME);
  scheme?.addEventListener("change", callback);
  return () => scheme?.removeEventListener("change", callback);
}
