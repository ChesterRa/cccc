import { useState, useEffect, useCallback, useMemo, useSyncExternalStore } from "react";
import { Theme } from "../types";
import { syncDocumentBrandingTheme } from "../utils/branding";
import { getAutomaticTheme, subscribeToAutomaticTheme } from "../utils/theme";

const THEME_STORAGE_KEY = "cccc-theme";

function getStoredTheme(): Theme {
  if (typeof window === "undefined") return "system";
  const stored = localStorage.getItem(THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") {
    return stored;
  }
  return "system";
}

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const effectiveTheme = theme === "system" ? getAutomaticTheme() : theme;

  // Remove both classes first
  root.classList.remove("light", "dark");

  // Add the appropriate class
  root.classList.add(effectiveTheme);

  // Update meta theme-color for mobile browsers
  const metaThemeColor = document.querySelector('meta[name="theme-color"]');
  if (metaThemeColor) {
    metaThemeColor.setAttribute("content", effectiveTheme === "dark" ? "#020617" : "#f8fafc");
  }
  syncDocumentBrandingTheme();
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme);

  const automaticTheme = useSyncExternalStore(
    subscribeToAutomaticTheme,
    getAutomaticTheme,
    () => "dark" as const,
  );

  // Compute resolvedTheme with useMemo to avoid extra state writes in effects.
  const resolvedTheme = useMemo<"light" | "dark">(
    () => (theme === "system" ? automaticTheme : theme),
    [theme, automaticTheme],
  );

  // Apply theme on mount and when theme changes
  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme, automaticTheme]);

  const setTheme = useCallback((newTheme: Theme) => {
    applyTheme(newTheme);
    localStorage.setItem(THEME_STORAGE_KEY, newTheme);
    setThemeState(newTheme);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((current) => {
      const nextTheme = current === "light" ? "dark" : current === "dark" ? "system" : "light";
      applyTheme(nextTheme);
      localStorage.setItem(THEME_STORAGE_KEY, nextTheme);
      return nextTheme;
    });
  }, []);

  return { theme, resolvedTheme, setTheme, toggleTheme, isDark: resolvedTheme === "dark" };
}
