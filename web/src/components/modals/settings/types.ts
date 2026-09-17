// Shared types/helpers for the Settings modal.
import { buttonVariants } from "../../ui/button-variants";
import { cn } from "../../../lib/utils";

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

// Settings share the same controls as the rest of the workbench.
export const inputClass = (_isDark?: boolean) =>
  "glass-input w-full rounded-lg px-3 py-2.5 text-sm leading-6 min-h-[44px] text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)] disabled:opacity-50 disabled:cursor-not-allowed";

export const labelClass = (_isDark?: boolean) =>
  "block text-sm mb-1.5 font-medium text-[var(--color-text-secondary)]";

// Keep localized actions wrappable and native disabled-button help reachable.
export const primaryButtonClass = (_busy?: boolean) =>
  cn(
    buttonVariants({ variant: "default" }),
    "min-w-0 whitespace-normal disabled:pointer-events-auto",
  );

export const secondaryButtonClass = (size: "sm" | "md" = "md") =>
  cn(
    buttonVariants({ variant: "secondary", size: size === "sm" ? "sm" : "default" }),
    "min-w-0 whitespace-normal disabled:pointer-events-auto",
  );

export const dangerButtonClass = (size: "sm" | "md" = "md") =>
  cn(
    buttonVariants({ variant: "destructive", size: size === "sm" ? "sm" : "default" }),
    "min-w-0 whitespace-normal disabled:pointer-events-auto",
  );

export const settingsDialogPanelClass = (size: "lg" | "xl" = "lg") =>
  `glass-modal absolute inset-0 sm:inset-auto sm:left-1/2 sm:top-1/2 ${
    size === "xl"
      ? "sm:w-[min(1200px,calc(100vw-2rem))] sm:h-[min(90dvh,920px)]"
      : "sm:w-[min(1040px,calc(100vw-2rem))] sm:h-[min(88dvh,860px)]"
  } sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-none sm:rounded-2xl shadow-2xl flex flex-col overflow-hidden`;

export const settingsDialogHeaderClass = `flex shrink-0 items-start gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5 sm:py-4`;

export const settingsDialogBodyClass = `min-h-0 flex-1 overflow-y-auto scrollbar-subtle p-4 sm:p-6 lg:p-7 [scrollbar-gutter:stable] [overflow-wrap:anywhere]`;

export const settingsDialogFooterClass = `flex shrink-0 items-center justify-end gap-2 border-t border-[var(--glass-border-subtle)] px-4 pt-3 pb-[calc(1rem+env(safe-area-inset-bottom,0px))] sm:px-5 sm:pt-4 sm:pb-[calc(1.25rem+env(safe-area-inset-bottom,0px))]`;

export const cardClass = (_isDark?: boolean) =>
  `glass-panel rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-4 shadow-sm`;

export const settingsWorkspaceShellClass = (_isDark?: boolean) =>
  "min-w-0 [overflow-wrap:anywhere]";

export const settingsWorkspaceHeaderClass = (_isDark?: boolean) =>
  "flex flex-wrap items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3 [&>div:first-child]:min-w-0 [&>div:first-child]:flex-1 [&>div:first-child]:basis-64";

export const settingsWorkspaceBodyClass = "py-4 space-y-4";

// Plain field groups do not need an additional visual container.
export const settingsWorkspaceFieldsClass = "min-w-0 space-y-3";

// Separate form sections without making every field group another card.
export const settingsWorkspaceSectionClass =
  "min-w-0 border-t border-[var(--glass-border-subtle)] pt-4 first:border-t-0 first:pt-0";

export const settingsWorkspacePanelClass = (_isDark?: boolean) =>
  "rounded-xl border border-[var(--glass-panel-border)] p-4 bg-[var(--color-bg-primary)]";

export const settingsWorkspaceSoftPanelClass = (_isDark?: boolean) =>
  `rounded-lg border border-[var(--glass-border-subtle)] px-3 py-3 bg-[var(--color-bg-secondary)]`;

export const settingsWorkspaceActionBarClass = (_isDark?: boolean) =>
  "flex flex-wrap items-center gap-2 border-t border-[var(--glass-border-subtle)] py-3";

export const preClass = (_isDark?: boolean) =>
  `mt-2 p-2 rounded overflow-x-auto whitespace-pre text-xs bg-[var(--color-bg-secondary)] text-[var(--color-text-primary)] border border-[var(--glass-border-subtle)]`;
