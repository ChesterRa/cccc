// Shared types/helpers for the Settings modal.

export type SettingsScope = "group" | "global";
export type GroupTabId =
  | "automation"
  | "delivery"
  | "guidance"
  | "assistants"
  | "space"
  | "messaging"
  | "connections"
  | "im"
  | "transcript"
  | "copyGroups";
export type GlobalTabId =
  | "account"
  | "capabilities"
  | "actorProfiles"
  | "myProfiles"
  | "branding"
  | "webAccess"
  | "webModels"
  | "developer";

// Shared style class helpers — glass design system
export const inputClass = (_isDark?: boolean) =>
  `glass-input w-full rounded-xl border border-black/[0.08] dark:border-white/[0.08] bg-white/60 dark:bg-white/[0.03] px-4 py-3 text-[var(--color-text-primary)] text-sm leading-6 min-h-[44px] placeholder:text-[var(--color-text-muted)] transition-all duration-200 focus:outline-none focus:border-black/20 dark:focus:border-white/20 focus:ring-2 focus:ring-slate-500/10 dark:focus:ring-white/5`;

export const labelClass = (_isDark?: boolean) =>
  `block text-xs mb-1.5 font-semibold tracking-wide text-[var(--color-text-secondary)]`;

export const primaryButtonClass = (_busy?: boolean) =>
  `inline-flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm min-h-[44px] font-semibold transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed border border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-black hover:border-black shadow-sm dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92 dark:hover:border-white active:scale-[0.97]`;

export const secondaryButtonClass = (size: "sm" | "md" = "md") =>
  `inline-flex items-center justify-center gap-2 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] hover:bg-[var(--glass-tab-bg-hover)] active:bg-[var(--glass-tab-bg-active)] active:scale-[0.97] ${
    size === "sm" ? "px-2.5 py-1.5 text-xs min-h-[36px]" : "px-3.5 py-2.5 text-sm min-h-[44px]"
  } cursor-pointer font-semibold text-[var(--color-text-primary)] shadow-sm transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed`;

export const dangerButtonClass = (size: "sm" | "md" = "md") =>
  `inline-flex items-center justify-center gap-2 rounded-xl border border-rose-500/20 bg-rose-500/10 hover:bg-rose-500/18 active:bg-rose-500/24 active:scale-[0.97] ${
    size === "sm" ? "px-2.5 py-1.5 text-xs min-h-[36px]" : "px-3.5 py-2.5 text-sm min-h-[44px]"
  } cursor-pointer font-semibold text-rose-600 dark:text-rose-400 shadow-sm transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed`;

export const settingsDialogPanelClass = (size: "lg" | "xl" = "lg") =>
  `glass-modal absolute inset-0 sm:inset-auto sm:left-1/2 sm:top-1/2 ${
    size === "xl"
      ? "sm:w-[min(1200px,calc(100vw-2rem))] sm:h-[min(90dvh,920px)]"
      : "sm:w-[min(1040px,calc(100vw-2rem))] sm:h-[min(88dvh,860px)]"
  } sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-none sm:rounded-2xl shadow-2xl flex flex-col overflow-hidden`;

export const settingsDialogHeaderClass = `flex shrink-0 items-start gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5 sm:py-4`;

export const settingsDialogBodyClass = `min-h-0 flex-1 overflow-y-auto scrollbar-subtle p-4 sm:p-6 lg:p-7 [scrollbar-gutter:stable]`;

export const settingsDialogFooterClass = `flex shrink-0 items-center justify-end gap-2 border-t border-[var(--glass-border-subtle)] px-4 pt-3 pb-[calc(1rem+env(safe-area-inset-bottom,0px))] sm:px-5 sm:pt-4 sm:pb-[calc(1.25rem+env(safe-area-inset-bottom,0px))]`;

export const cardClass = (_isDark?: boolean) =>
  `glass-panel rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4 shadow-sm`;

export const settingsWorkspaceShellClass = (_isDark?: boolean) => "min-w-0";

export const settingsWorkspaceHeaderClass = (_isDark?: boolean) =>
  "flex flex-wrap items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3";

export const settingsWorkspaceBodyClass = "py-4 space-y-4";

// Plain field groups do not need an additional visual container.
export const settingsWorkspaceFieldsClass = "min-w-0 space-y-3";

export const settingsWorkspacePanelClass = (_isDark?: boolean) =>
  `rounded-xl border border-[var(--glass-border-subtle)] p-4 bg-[var(--color-bg-primary)]`;

export const settingsWorkspaceSoftPanelClass = (_isDark?: boolean) =>
  `rounded-lg border border-[var(--glass-border-subtle)] px-3 py-3 bg-[var(--color-bg-secondary)]`;

export const settingsWorkspaceActionBarClass = (_isDark?: boolean) =>
  "flex flex-wrap items-center gap-2 border-t border-[var(--glass-border-subtle)] py-3";

export const preClass = (_isDark?: boolean) =>
  `mt-2 p-2 rounded overflow-x-auto whitespace-pre text-[11px] bg-[var(--color-bg-secondary)] text-[var(--color-text-primary)] border border-[var(--glass-border-subtle)]`;
