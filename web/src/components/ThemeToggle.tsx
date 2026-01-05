import { Theme } from "../types";
import { classNames } from "../utils/classNames";
import { SunIcon, MoonIcon, TerminalIcon } from "./Icons";

interface ThemeToggleProps {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  isDark: boolean;
}

export function ThemeToggle({ theme, onThemeChange, isDark }: ThemeToggleProps) {
  const themes: { value: Theme; label: string; Icon: React.FC<{ size?: number }> }[] = [
    { value: "light", label: "Light", Icon: SunIcon },
    { value: "dark", label: "Dark", Icon: MoonIcon },
    { value: "system", label: "System", Icon: TerminalIcon },
  ];

  return (
    <div className="flex items-center gap-1 p-1 rounded-xl glass-btn">
      {themes.map((t) => (
        <button
          key={t.value}
          onClick={() => onThemeChange(t.value)}
          className={classNames(
            "flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all min-h-[36px]",
            theme === t.value
              ? isDark
                ? "bg-white/10 text-white shadow-sm"
                : "bg-black/5 text-gray-900 shadow-sm"
              : isDark
                ? "text-slate-400 hover:text-slate-200"
                : "text-gray-500 hover:text-gray-700"
          )}
          aria-label={`Switch to ${t.label} theme`}
          aria-pressed={theme === t.value}
        >
          <t.Icon size={14} />
          <span className="hidden sm:inline">{t.label}</span>
        </button>
      ))}
    </div>
  );
}

// Compact version for header
export function ThemeToggleCompact({ theme, onThemeChange, isDark }: ThemeToggleProps) {
  const nextTheme = (): Theme => {
    if (theme === "light") return "dark";
    if (theme === "dark") return "system";
    return "light";
  };

  const Icon = theme === "light" ? SunIcon : theme === "dark" ? MoonIcon : TerminalIcon;
  const label = theme === "light" ? "Light" : theme === "dark" ? "Dark" : "System";

  return (
    <button
      onClick={() => onThemeChange(nextTheme())}
      className={classNames(
        "flex items-center justify-center w-9 h-9 rounded-xl transition-all min-h-[44px] min-w-[44px] glass-btn",
        isDark ? "text-slate-300" : "text-gray-600"
      )}
      title={`Theme: ${label}. Click to change.`}
      aria-label={`Current theme: ${label}. Click to cycle through themes.`}
    >
      <Icon size={18} />
    </button>
  );
}
