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
}

export function createContextModalUi(_isDark: boolean): ContextModalUi {
  const mutedTextClass = "text-[var(--color-text-muted)]";
  const subtleTextClass = "text-[var(--color-text-secondary)]";
  const inputClass = classNames(
    "w-full rounded-xl px-4 py-2.5 text-sm outline-none transition-colors min-h-[44px]",
    "glass-input text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)]",
  );
  const textareaClass = classNames(inputClass, "min-h-[96px] resize-y px-3 py-2");
  const buttonSecondaryClass = classNames(
    buttonVariants({ variant: "outline" }),
    "whitespace-normal",
  );
  const buttonPrimaryClass = classNames(
    buttonVariants({ variant: "default" }),
    "whitespace-normal",
  );
  const buttonDangerClass = classNames(
    buttonVariants({ variant: "destructive" }),
    "whitespace-normal",
  );

  return {
    surfaceClass: classNames(
      "rounded-xl border border-[var(--glass-panel-border)] bg-[var(--color-bg-primary)]",
    ),
    mutedTextClass,
    subtleTextClass,
    inputClass,
    textareaClass,
    buttonSecondaryClass,
    buttonPrimaryClass,
    buttonDangerClass,
    chipBaseClass: classNames(
      "min-h-9 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
    ),
  };
}
