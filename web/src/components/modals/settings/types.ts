// Shared types/helpers for the Settings modal.

export type SettingsScope = "group" | "global";
export type GroupTabId =
  | "automation"
  | "delivery"
  | "guidance"
  | "messaging"
  | "im"
  | "transcript"
  | "blueprint";
export type GlobalTabId = "remote" | "developer";

// Shared style class helpers
export const inputClass = (isDark: boolean) =>
  `w-full px-3 py-2.5 rounded-lg border text-sm min-h-[44px] transition-colors ${
    isDark
      ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
      : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
  }`;

export const labelClass = (isDark: boolean) =>
  `block text-xs mb-1 ${isDark ? "text-slate-400" : "text-gray-500"}`;

export const primaryButtonClass = (_busy?: boolean) =>
  `px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors font-medium`;

export const cardClass = (isDark: boolean) =>
  `rounded-lg border p-3 ${isDark ? "border-slate-800 bg-slate-950/30" : "border-gray-200 bg-gray-50"}`;

export const preClass = (isDark: boolean) =>
  `mt-2 p-2 rounded overflow-x-auto whitespace-pre text-[11px] ${
    isDark ? "bg-slate-900 text-slate-200" : "bg-white text-gray-800 border border-gray-200"
  }`;
