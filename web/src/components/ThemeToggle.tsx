import { useCallback, useRef, type ComponentType } from "react";
import { flushSync } from "react-dom";
import { useTranslation } from "react-i18next";
import { Theme } from "../types";
import { classNames } from "../utils/classNames";
import { SunIcon, MoonIcon, MonitorIcon } from "./Icons";

interface ThemeToggleProps {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  isDark: boolean;
  variant?: "default" | "rail";
  className?: string;
}

type ViewTransition = { ready?: Promise<unknown> };

type DocumentWithViewTransition = Document & {
  startViewTransition?: (update: () => void) => ViewTransition;
};

function getNextTheme(theme: Theme): Theme {
  if (theme === "light") return "dark";
  if (theme === "dark") return "system";
  return "light";
}

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function getEffectiveTheme(theme: Theme): "light" | "dark" {
  return theme === "system" ? getSystemTheme() : theme;
}

export function ThemeToggle({ theme, onThemeChange, isDark: _isDark }: ThemeToggleProps) {
  const { t } = useTranslation("layout");
  const themes: {
    value: Theme;
    label: string;
    Icon: ComponentType<{ className?: string; size?: number }>;
  }[] = [
    { value: "light", label: t("themeLight"), Icon: SunIcon },
    { value: "dark", label: t("themeDark"), Icon: MoonIcon },
    { value: "system", label: t("themeSystem"), Icon: MonitorIcon },
  ];
  const activeIndex = themes.findIndex((item) => item.value === theme);

  return (
    <div className="relative grid grid-cols-3 gap-1 rounded-2xl glass-btn p-1">
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-y-1 left-1 w-[calc((100%-0.5rem)/3)] rounded-[14px] bg-black/[0.055] shadow-[0_8px_24px_rgba(15,23,42,0.08)] transition-transform duration-300 ease-out dark:bg-white/[0.12] dark:shadow-[0_10px_26px_rgba(2,6,23,0.32)]"
        style={{
          transform: `translateX(calc(${Math.max(activeIndex, 0)} * 100% + ${Math.max(activeIndex, 0)} * 0.25rem))`,
        }}
      />
      {themes.map((th) => {
        const selected = theme === th.value;
        return (
          <button
            key={th.value}
            onClick={() => onThemeChange(th.value)}
            className={classNames(
              "relative z-[1] flex items-center justify-center gap-1.5 rounded-[14px] px-2.5 py-1.5 text-xs font-medium transition-all min-h-[36px]",
              selected
                ? "text-gray-900 dark:text-white"
                : "text-gray-500 hover:text-gray-700 dark:text-[var(--color-text-secondary)] dark:hover:text-[var(--color-text-primary)]",
            )}
            aria-label={t("switchToTheme", { theme: th.label })}
            aria-pressed={selected}
          >
            <span
              className={classNames(
                "flex h-4 w-4 items-center justify-center transition-transform duration-300 ease-out",
                selected ? "scale-100" : "scale-90",
              )}
            >
              <th.Icon size={14} />
            </span>
            <span className="hidden sm:inline">{th.label}</span>
          </button>
        );
      })}
    </div>
  );
}

// Compact version for header
export function ThemeToggleCompact({
  theme,
  onThemeChange,
  isDark: _isDark,
  variant = "default",
  className,
}: ThemeToggleProps) {
  const { t } = useTranslation("layout");
  const buttonRef = useRef<HTMLButtonElement | null>(null);

  const label =
    theme === "light" ? t("themeLight") : theme === "dark" ? t("themeDark") : t("themeSystem");
  const nextTheme = getNextTheme(theme);
  const nextLabel =
    nextTheme === "light"
      ? t("themeLight")
      : nextTheme === "dark"
        ? t("themeDark")
        : t("themeSystem");

  const handleToggle = useCallback(() => {
    const button = buttonRef.current;
    const currentEffectiveTheme = getEffectiveTheme(theme);
    const nextEffectiveTheme = getEffectiveTheme(nextTheme);
    const documentWithTransition = document as DocumentWithViewTransition;

    if (
      !button ||
      typeof window === "undefined" ||
      typeof documentWithTransition.startViewTransition !== "function" ||
      currentEffectiveTheme === nextEffectiveTheme
    ) {
      onThemeChange(nextTheme);
      return;
    }

    const { left, top, width, height } = button.getBoundingClientRect();
    const x = left + width / 2;
    const y = top + height / 2;
    const viewportWidth = window.visualViewport?.width ?? window.innerWidth;
    const viewportHeight = window.visualViewport?.height ?? window.innerHeight;
    const maxRadius = Math.hypot(Math.max(x, viewportWidth - x), Math.max(y, viewportHeight - y));

    const transition = documentWithTransition.startViewTransition(() => {
      flushSync(() => onThemeChange(nextTheme));
    });

    transition.ready?.then(() => {
      document.documentElement.animate(
        { clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${maxRadius}px at ${x}px ${y}px)`] },
        {
          duration: 420,
          easing: "cubic-bezier(0.65, 0, 0.35, 1)",
          pseudoElement: "::view-transition-new(root)",
        },
      );
    });
  }, [nextTheme, onThemeChange, theme]);

  return (
    <button
      ref={buttonRef}
      onClick={handleToggle}
      className={classNames(
        variant === "rail"
          ? "group relative flex items-center justify-center h-9 w-9 min-h-[36px] min-w-[36px] rounded-[14px] transition-all shrink-0 border border-transparent bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
          : "group relative flex items-center justify-center w-11 h-11 rounded-xl transition-all min-h-[44px] min-w-[44px] shrink-0 glass-btn text-[var(--color-text-secondary)]",
        className,
      )}
      title={t("switchToTheme", { theme: nextLabel })}
      aria-label={t("currentTheme", { theme: label })}
      aria-live="polite"
    >
      <span className="relative h-[18px] w-[18px] overflow-hidden">
        <span
          className={classNames(
            "absolute inset-0 flex items-center justify-center transition-all duration-300 ease-out",
            theme === "light"
              ? "translate-y-0 scale-100 opacity-100 rotate-0"
              : "-translate-y-5 scale-75 opacity-0 -rotate-45",
          )}
        >
          <SunIcon size={17} />
        </span>
        <span
          className={classNames(
            "absolute inset-0 flex items-center justify-center transition-all duration-300 ease-out",
            theme === "dark"
              ? "translate-y-0 scale-100 opacity-100 rotate-0"
              : theme === "system"
                ? "translate-y-5 scale-75 opacity-0 rotate-45"
                : "translate-y-5 scale-75 opacity-0 rotate-45",
          )}
        >
          <MoonIcon size={17} />
        </span>
        <span
          className={classNames(
            "absolute inset-0 flex items-center justify-center transition-all duration-300 ease-out",
            theme === "system"
              ? "translate-y-0 scale-100 opacity-100 rotate-0"
              : theme === "light"
                ? "translate-y-5 scale-75 opacity-0 rotate-45"
                : "-translate-y-5 scale-75 opacity-0 -rotate-45",
          )}
        >
          <MonitorIcon size={17} />
        </span>
      </span>
    </button>
  );
}
