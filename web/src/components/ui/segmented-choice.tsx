import { useRef } from "react";
import { classNames } from "../../utils/classNames";

export type SegmentedOption<Value> = { value: Value; label: string };

/**
 * A segmented radio group: every option is visible and one tap applies it.
 * Arrow keys move the selection, as in any radio group.
 */
export function SegmentedChoice<Value extends string | number>({
  value,
  options,
  onChange,
  labelledBy,
  ariaLabel,
  fill = true,
  disabled = false,
  ...rest
}: {
  value: Value;
  options: SegmentedOption<Value>[];
  onChange: (value: Value) => void;
  labelledBy?: string;
  ariaLabel?: string;
  /** Spread the segments across the full width instead of sizing them to their labels. */
  fill?: boolean;
  disabled?: boolean;
} & Record<`data-${string}`, string | undefined>) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = options.findIndex((option) => option.value === value);
  const move = (index: number) => {
    const next = (index + options.length) % options.length;
    onChange(options[next].value);
    buttons.current[next]?.focus();
  };
  return (
    <div
      role="radiogroup"
      aria-labelledby={labelledBy}
      aria-label={ariaLabel}
      aria-disabled={disabled || undefined}
      {...rest}
      className={classNames(
        "rounded-[10px] bg-[var(--glass-tab-bg)] p-[3px]",
        fill ? "grid" : "inline-flex max-w-full flex-wrap",
      )}
      style={
        fill ? { gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` } : undefined
      }
    >
      {options.map((option, index) => {
        const checked = index === selected;
        return (
          <button
            key={String(option.value)}
            ref={(element) => {
              buttons.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            disabled={disabled}
            tabIndex={checked || (selected < 0 && index === 0) ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => {
              if (event.key === "ArrowRight" || event.key === "ArrowDown") {
                event.preventDefault();
                move(index + 1);
              } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
                event.preventDefault();
                move(index - 1);
              }
            }}
            className={classNames(
              "h-[30px] min-w-0 truncate rounded-lg px-3 text-[13px] transition-colors focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--color-text-secondary)] disabled:opacity-50 pointer-coarse:h-10",
              checked
                ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-[0_1px_2px_rgba(24,24,27,0.08),0_0_0_1px_rgba(24,24,27,0.04)]"
                : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
