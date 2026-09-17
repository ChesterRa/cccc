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
        className="pointer-events-none relative h-7 w-12 rounded-full border border-[var(--color-border-secondary)] bg-[var(--color-bg-secondary)] transition-colors peer-checked:border-[var(--primary)] peer-checked:bg-[var(--primary)] peer-focus-visible:shadow-[var(--focus-ring)] peer-disabled:opacity-50 after:absolute after:left-px after:top-1/2 after:h-6 after:w-6 after:-translate-y-1/2 after:rounded-full after:bg-[var(--color-text-secondary)] after:transition-transform peer-checked:after:translate-x-5 peer-checked:after:bg-[var(--primary-foreground)] motion-reduce:after:transition-none"
      />
    </span>
  );
}
