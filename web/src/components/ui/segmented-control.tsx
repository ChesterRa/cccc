import { useRef } from "react";

import { cn } from "@/lib/utils";

export type SegmentedOption<T extends string> = { value: T; label: string };

/** A compact single-choice control with radio semantics and arrow-key movement. */
export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  disabled = false,
  onChange,
  className,
}: {
  label: string;
  value: T;
  options: ReadonlyArray<SegmentedOption<T>>;
  disabled?: boolean;
  onChange: (value: T) => void;
  className?: string;
}) {
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  const selected = options.findIndex((option) => option.value === value);
  const select = (index: number) => {
    const next = (index + options.length) % options.length;
    onChange(options[next].value);
    buttons.current[next]?.focus();
  };
  return (
    <div
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      className={cn(
        "inline-flex max-w-full items-center gap-0.5 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] p-0.5",
        disabled && "opacity-60",
        className,
      )}
    >
      {options.map((option, index) => {
        const checked = index === selected;
        return (
          <button
            key={option.value}
            ref={(node) => {
              buttons.current[index] = node;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={checked || (selected < 0 && index === 0) ? 0 : -1}
            disabled={disabled}
            onClick={() => {
              if (!checked) onChange(option.value);
            }}
            onKeyDown={(event) => {
              if (event.key === "ArrowRight" || event.key === "ArrowDown") {
                event.preventDefault();
                select(index + 1);
              } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
                event.preventDefault();
                select(index - 1);
              }
            }}
            className={cn(
              "min-h-8 flex-1 whitespace-nowrap rounded-md px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] disabled:cursor-not-allowed",
              checked
                ? "bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] shadow-sm"
                : "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
