import { classNames } from "../../utils/classNames";
import { buttonVariants } from "../ui/button-variants";

export interface ContextModalUi {
  surfaceClass: string;
  mutedTextClass: string;
  subtleTextClass: string;
  inputClass: string;
  textareaClass: string;
  buttonSecondaryClass: string;
  buttonPrimaryClass: string;
  buttonDangerClass: string;
  chipBaseClass: string;
  switchTrackClass: (active: boolean) => string;
  switchThumbClass: (active: boolean) => string;
}

export function createContextModalUi(isDark: boolean): ContextModalUi {
  const mutedTextClass = "text-[var(--color-text-muted)]";
  const subtleTextClass = "text-[var(--color-text-secondary)]";
  const inputClass = classNames(
    "w-full rounded-xl px-4 py-2.5 text-sm outline-none transition-colors min-h-[44px]",
    "glass-input text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)]",
  );
  const textareaClass = classNames(inputClass, "min-h-[96px] resize-y px-3 py-2");
  const buttonSecondaryClass = classNames(
    buttonVariants({ variant: "outline" }),
    "border-black/10 bg-white/84 text-[rgb(35,36,37)] shadow-[0_10px_24px_-22px_rgba(15,23,42,0.35)] hover:bg-white",
    "dark:border-white/12 dark:bg-white/[0.06] dark:text-white dark:hover:bg-white/[0.1]",
  );
  const buttonPrimaryClass = classNames(
    buttonVariants({ variant: "default" }),
    "bg-[rgb(35,36,37)] text-white shadow-[0_18px_34px_-22px_rgba(15,23,42,0.52)] hover:bg-black",
    "dark:bg-white dark:text-[rgb(20,20,22)] dark:hover:bg-white/92",
  );
  const buttonDangerClass = classNames(
    buttonVariants({ variant: "destructive" }),
    "border-rose-500/30 bg-rose-500/12 text-rose-600 hover:bg-rose-500/18 dark:text-rose-300",
  );

  return {
    surfaceClass: classNames(
      "rounded-2xl border shadow-sm",
      "glass-panel border-[var(--glass-panel-border)]",
    ),
    mutedTextClass,
    subtleTextClass,
    inputClass,
    textareaClass,
    buttonSecondaryClass,
    buttonPrimaryClass,
    buttonDangerClass,
    chipBaseClass: classNames(
      "rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
      "glass-card border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)]",
    ),
    switchTrackClass: (active: boolean) =>
      classNames(
        "relative inline-flex h-6 w-11 shrink-0 rounded-full border transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        active
          ? isDark
            ? "border-white/16 bg-white/18"
            : "border-black/12 bg-[rgb(35,36,37)]"
          : isDark
            ? "border-slate-700 bg-slate-900"
            : "border-gray-300 bg-gray-200",
      ),
    switchThumbClass: (active: boolean) =>
      classNames(
        "pointer-events-none inline-block h-5 w-5 rounded-full shadow-sm transition-transform",
        active ? (isDark ? "bg-white" : "bg-white") : isDark ? "bg-slate-500" : "bg-white",
        active ? "translate-x-5" : "translate-x-0",
      ),
  };
}
