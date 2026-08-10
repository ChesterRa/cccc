import { cva } from "class-variance-authority";

export const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-xl font-medium transition-[background-color,border-color,color,box-shadow,transform] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-45 disabled:active:scale-100",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--primary)] text-[var(--primary-foreground)] shadow-sm hover:brightness-95",
        secondary: "glass-btn text-[var(--color-text-secondary)]",
        destructive:
          "border border-[color-mix(in_srgb,var(--destructive)_30%,transparent)] bg-[color-mix(in_srgb,var(--destructive)_12%,transparent)] text-[var(--destructive)] hover:bg-[color-mix(in_srgb,var(--destructive)_18%,transparent)]",
        outline:
          "border border-[var(--glass-border-subtle)] bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)]",
        ghost:
          "border border-transparent bg-transparent text-[var(--color-text-secondary)] shadow-none hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
      },
      size: {
        default: "min-h-[44px] px-4 py-2.5 text-sm",
        sm: "min-h-[36px] rounded-lg px-3 py-1.5 text-xs",
        lg: "min-h-[52px] px-5 py-3 text-base",
        icon: "h-10 w-10 shrink-0",
        iconSm: "h-8 w-8 shrink-0 rounded-lg",
        iconRail: "h-9 w-9 shrink-0",
        iconTouch: "h-11 w-11 shrink-0",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);
