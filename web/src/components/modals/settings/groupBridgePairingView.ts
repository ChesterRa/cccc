import type { GroupBridgeAccessLevel } from "../../../services/api/groupBridge";

export function defaultIssuerEndpoint(): string {
  return typeof window !== "undefined" ? window.location.origin : "";
}

export function accessButtonClass(level: GroupBridgeAccessLevel, selected: boolean): string {
  const selectedClass =
    level === "full"
      ? "border-rose-500/40 bg-rose-500/15 text-rose-700 dark:text-rose-200"
      : level === "read"
        ? "border-sky-500/35 bg-sky-500/15 text-sky-700 dark:text-sky-200"
        : "border-slate-500/25 bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]";
  return [
    "min-h-[32px] rounded-lg border px-2.5 py-1.5 text-xs font-semibold transition-all duration-150",
    "focus:outline-none focus:ring-2 focus:ring-slate-500/15",
    "disabled:cursor-not-allowed disabled:opacity-50",
    selected
      ? selectedClass
      : "border-transparent text-[var(--color-text-muted)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
  ].join(" ");
}
