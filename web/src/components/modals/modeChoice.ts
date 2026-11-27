/**
 * Props for a two-way mode choice rendered as a pair of buttons (agent role, runtime
 * configuration mode). The pressed state is exposed to assistive technology, and
 * data-mode-choice lets a style (previewTheme.css) restyle the pair as one family.
 */
export function modeChoiceProps(selected: boolean) {
  return {
    "aria-pressed": selected,
    "data-mode-choice": "",
    className: [
      "min-w-0 whitespace-normal px-3 py-2.5 rounded-xl border text-sm min-h-[44px] font-medium transition-all ease-spring duration-200",
      selected
        ? "border-primary/40 bg-primary/10 text-primary shadow-sm dark:border-white/25 dark:bg-white/14 dark:text-white dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.2)]"
        : "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
    ].join(" "),
  };
}
