import { useState, useEffect, useCallback, useMemo, useSyncExternalStore } from "react";
import { Theme } from "../types";

const THEME_STORAGE_KEY = "cccc-theme";

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

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
  const effectiveTheme = theme === "system" ? getSystemTheme() : theme;
  
  // Remove both classes first
  root.classList.remove("light", "dark");
  
  // Add the appropriate class
  root.classList.add(effectiveTheme);
  
  // Update meta theme-color for mobile browsers
  const metaThemeColor = document.querySelector('meta[name="theme-color"]');
  if (metaThemeColor) {
    metaThemeColor.setAttribute(
      "content",
      effectiveTheme === "dark" ? "#020617" : "#f8fafc"
    );
  }
}

// 订阅系统主题变化
function subscribeToSystemTheme(callback: () => void) {
  const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  mediaQuery.addEventListener("change", callback);
  return () => mediaQuery.removeEventListener("change", callback);
}

export function useTheme() {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme);
  
  // 使用 useSyncExternalStore 订阅系统主题变化
  const systemTheme = useSyncExternalStore(
    subscribeToSystemTheme,
    getSystemTheme,
    () => "dark" as const
  );

  // 使用 useMemo 计算 resolvedTheme，避免在 effect 中 setState
  const resolvedTheme = useMemo<"light" | "dark">(
    () => (theme === "system" ? systemTheme : theme),
    [theme, systemTheme]
  );

  // Apply theme on mount and when theme changes
  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme, systemTheme]);

  const setTheme = useCallback((newTheme: Theme) => {
    setThemeState(newTheme);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((current) => {
      if (current === "light") return "dark";
      if (current === "dark") return "system";
      return "light";
    });
  }, []);

  return {
    theme,
    resolvedTheme,
    setTheme,
    toggleTheme,
    isDark: resolvedTheme === "dark",
  };
}

// Terminal theme colors based on CSS variables
export function getTerminalTheme(isDark: boolean) {
  return {
    background: isDark ? "#0f172a" : "#fafafa",
    foreground: isDark ? "#e2e8f0" : "#1e293b",
    cursor: isDark ? "#e2e8f0" : "#1e293b",
    cursorAccent: isDark ? "#0f172a" : "#fafafa",
    selectionBackground: isDark ? "#334155" : "#bfdbfe",
    black: isDark ? "#1e293b" : "#64748b",
    red: isDark ? "#f87171" : "#dc2626",
    green: isDark ? "#4ade80" : "#16a34a",
    yellow: isDark ? "#facc15" : "#ca8a04",
    blue: isDark ? "#60a5fa" : "#2563eb",
    magenta: isDark ? "#c084fc" : "#9333ea",
    cyan: isDark ? "#22d3ee" : "#0891b2",
    white: isDark ? "#f1f5f9" : "#f1f5f9",
    brightBlack: isDark ? "#475569" : "#94a3b8",
    brightRed: isDark ? "#fca5a5" : "#ef4444",
    brightGreen: isDark ? "#86efac" : "#22c55e",
    brightYellow: isDark ? "#fde047" : "#eab308",
    brightBlue: isDark ? "#93c5fd" : "#3b82f6",
    brightMagenta: isDark ? "#d8b4fe" : "#a855f7",
    brightCyan: isDark ? "#67e8f9" : "#06b6d4",
    brightWhite: isDark ? "#f8fafc" : "#f8fafc",
  };
}
