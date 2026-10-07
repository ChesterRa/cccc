// Settings switch between a phone layout and a sidebar layout by the panel's
// own width (container `voice-settings`), not the window: the panel lives in a modal.
// Class names stay literal so Tailwind can see them.

/** Content column: left-aligned, wide enough to avoid an empty right side. */
export const SETTINGS_COLUMN = "w-full max-w-[1120px] px-4 @min-[900px]/voice-settings:px-12";

export const SETTINGS_CARD =
  "rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]";
