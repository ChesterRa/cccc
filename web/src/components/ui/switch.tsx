import type { InputHTMLAttributes } from "react";

import { cn } from "@/lib/utils";

// Keep a compact track with a full native keyboard and touch target.
export function Switch({
  className,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type">) {
  return (
    <span className={cn("relative inline-flex min-h-[44px] w-12 shrink-0 items-center", className)}>
      <input
        {...props}
        type="checkbox"
        role="switch"
        className="peer absolute inset-0 z-10 m-0 h-full w-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
      />
      <span
        aria-hidden="true"
        data-switch-track
        className="pointer-events-none relative h-7 w-12 rounded-full border border-[var(--color-border-primary)] bg-[var(--color-bg-secondary)] transition-colors peer-checked:border-[var(--primary)] peer-focus-visible:shadow-[var(--focus-ring)] peer-disabled:opacity-50 after:absolute after:left-1 after:top-1/2 after:box-border after:h-[18px] after:w-[18px] after:-translate-y-1/2 after:rounded-full after:border after:border-[var(--color-text-secondary)] after:bg-[var(--color-bg-elevated)] after:transition-transform peer-checked:after:translate-x-5 peer-checked:after:border-[var(--primary)] peer-checked:after:bg-[var(--primary)] motion-reduce:after:transition-none"
      />
    </span>
  );
}
