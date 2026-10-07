import { classNames } from "../../utils/classNames";
import type { PresentationWebPreviewMode } from "./PresentationWebPreviewPanel";

/** Standard/Enhanced switch for web previews; the active option reads as raised. */
export function PresentationPreviewModeToggle({
  value,
  label,
  options,
  compact = false,
  onChange,
}: {
  value: PresentationWebPreviewMode;
  label: string;
  options: Record<PresentationWebPreviewMode, { label: string; help: string }>;
  compact?: boolean;
  onChange: (mode: PresentationWebPreviewMode) => void;
}) {
  return (
    <div
      className="inline-flex items-center gap-0.5 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] p-0.5"
      role="group"
      aria-label={label}
    >
      {(["embedded", "interactive"] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          onClick={() => onChange(mode)}
          className={classNames(
            "rounded-md text-xs font-medium whitespace-nowrap transition-colors",
            compact ? "px-2 py-0.5" : "px-2.5 py-1 max-sm:min-h-9 max-sm:px-3.5 max-sm:text-[13px]",
            value === mode
              ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-sm"
              : "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
          )}
          aria-pressed={value === mode}
          title={options[mode].help}
        >
          {options[mode].label}
        </button>
      ))}
    </div>
  );
}
