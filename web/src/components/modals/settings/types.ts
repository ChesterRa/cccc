// Shared types/helpers for the Settings modal.

export type SettingsScope = "group" | "global";
export type GroupTabId =
  | "automation"
  | "delivery"
  | "guidance"
  | "assistants"
  | "space"
  | "messaging"
  | "im"
  | "transcript"
  | "blueprint";
export type GlobalTabId =
  | "capabilities"
  | "actorProfiles"
  | "myProfiles"
  | "branding"
  | "webAccess"
  | "webModels"
  | "developer";

// Shared style class helpers — glass design system
export const inputClass = (_isDark?: boolean) =>
  `glass-input w-full rounded-xl px-4 py-3 pr-5 text-[var(--color-text-primary)] text-sm leading-6 min-h-[44px] placeholder:text-[var(--color-text-muted)] focus-visible:outline-none focus-visible:border-[rgba(15,23,42,0.12)] focus-visible:shadow-[0_0_0_3px_rgba(148,163,184,0.16)] dark:focus-visible:border-white/12 dark:focus-visible:shadow-[0_0_0_3px_rgba(255,255,255,0.08)]`;

export const labelClass = (_isDark?: boolean) =>
  `block text-xs mb-1 text-[var(--color-text-secondary)]`;

export const primaryButtonClass = (_busy?: boolean) =>
  `inline-flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm min-h-[44px] font-medium transition-[background-color,border-color,color,box-shadow] disabled:opacity-50 disabled:cursor-not-allowed border border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-black hover:border-black shadow-sm dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92 dark:hover:border-white`;

export const secondaryButtonClass = (size: "sm" | "md" = "md") =>
  `inline-flex items-center justify-center gap-2 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] hover:bg-[var(--glass-bg-hover)] active:bg-[var(--glass-bg-active)] ${
    size === "sm" ? "px-2.5 py-1.5 text-xs min-h-[36px]" : "px-3.5 py-2.5 text-sm min-h-[44px]"
  } cursor-pointer font-medium text-[rgb(35,36,37)] dark:text-white shadow-sm transition-[background-color,border-color,box-shadow,color] disabled:opacity-50 disabled:cursor-not-allowed`;

export const dangerButtonClass = (size: "sm" | "md" = "md") =>
  `inline-flex items-center justify-center gap-2 rounded-xl border border-rose-500/30 bg-rose-500/15 hover:bg-rose-500/22 active:bg-rose-500/28 ${
    size === "sm" ? "px-2.5 py-1.5 text-xs min-h-[36px]" : "px-3.5 py-2.5 text-sm min-h-[44px]"
  } cursor-pointer font-medium text-rose-700 dark:text-rose-300 shadow-sm transition-[background-color,border-color,box-shadow] disabled:opacity-50 disabled:cursor-not-allowed`;

export const settingsDialogPanelClass = (size: "lg" | "xl" = "lg") =>
  `glass-modal absolute inset-0 sm:inset-auto sm:left-1/2 sm:top-1/2 ${
    size === "xl"
      ? "sm:w-[min(1200px,calc(100vw-2rem))] sm:h-[min(90dvh,920px)]"
      : "sm:w-[min(1040px,calc(100vw-2rem))] sm:h-[min(88dvh,860px)]"
  } sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-none sm:rounded-2xl shadow-2xl flex flex-col overflow-hidden`;

export const settingsDialogHeaderClass =
  `flex shrink-0 items-start gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5 sm:py-4`;

export const settingsDialogBodyClass =
  `min-h-0 flex-1 overflow-y-auto scrollbar-subtle p-4 sm:p-6 lg:p-7 [scrollbar-gutter:stable]`;

export const settingsDialogFooterClass =
  `flex shrink-0 items-center justify-end gap-2 border-t border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5 sm:py-4 safe-area-bottom-compact`;

export const cardClass = (_isDark?: boolean) =>
  `glass-panel rounded-lg p-3`;

export const settingsWorkspaceShellClass = (_isDark?: boolean) =>
  `overflow-hidden rounded-[22px] border backdrop-blur-xl ${
    _isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(19,20,24,0.88),rgba(10,11,14,0.96))] shadow-[0_28px_100px_rgba(0,0,0,0.36)]"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(246,248,251,0.96))] shadow-[0_28px_100px_rgba(15,23,42,0.06)]"
  }`;

export const settingsWorkspaceHeaderClass = (_isDark?: boolean) =>
  `flex items-start justify-between gap-4 px-4 py-4 sm:px-5 sm:py-4 ${
    _isDark ? "border-b border-white/8 bg-white/[0.03]" : "border-b border-black/6 bg-[rgba(18,18,20,0.018)]"
  }`;

export const settingsWorkspaceBodyClass =
  `px-4 py-4 sm:px-5 sm:py-5 space-y-4`;

export const settingsWorkspacePanelClass = (_isDark?: boolean) =>
  `rounded-[18px] border p-3.5 sm:p-4 ${
    _isDark
      ? "border-white/10 bg-[linear-gradient(180deg,rgba(24,26,31,0.9),rgba(13,14,18,0.98))]"
      : "border-black/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.995),rgba(246,248,251,0.96))]"
  }`;

export const settingsWorkspaceSoftPanelClass = (_isDark?: boolean) =>
  `rounded-[18px] border px-4 py-3 sm:px-4 sm:py-4 ${
    _isDark
      ? "border-white/8 bg-white/[0.03]"
      : "border-black/6 bg-[linear-gradient(180deg,rgba(255,255,255,0.94),rgba(246,248,251,0.88))]"
  }`;

export const settingsWorkspaceActionBarClass = (_isDark?: boolean) =>
  `mt-0 flex flex-wrap items-center gap-2 border-t px-4 py-3 sm:px-5 ${
    _isDark ? "border-white/8 bg-white/[0.02]" : "border-black/6 bg-black/[0.015]"
  }`;

export const preClass = (_isDark?: boolean) =>
  `mt-2 p-2 rounded overflow-x-auto whitespace-pre text-[11px] bg-[var(--color-bg-secondary)] text-[var(--color-text-primary)] border border-[var(--glass-border-subtle)]`;
