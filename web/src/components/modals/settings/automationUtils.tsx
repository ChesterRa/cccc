import React from "react";

import type { AutomationRule, AutomationRuleAction } from "../../../types";
import { cardClass, inputClass, labelClass } from "./types";

export const BellIcon = ({ className }: { className?: string }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
    <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
  </svg>
);

export const SparkIcon = ({ className }: { className?: string }) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M12 2l1.5 6L20 10l-6.5 2L12 18l-1.5-6L4 10l6.5-2L12 2z" />
  </svg>
);

export const formatDuration = (secondsRaw: number): string => {
  const seconds = Number.isFinite(secondsRaw) ? Math.max(0, Math.trunc(secondsRaw)) : 0;
  if (seconds <= 0) return "Off";
  const parts: string[] = [];
  let rem = seconds;
  const units: Array<[number, string]> = [
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
    [1, "s"],
  ];
  for (const [unit, label] of units) {
    if (rem < unit) continue;
    const v = Math.floor(rem / unit);
    rem -= v * unit;
    parts.push(`${v}${label}`);
    if (parts.length >= 2) break;
  }
  return parts.join(" ");
};

export const Section = ({
  isDark,
  icon: Icon,
  title,
  description,
  children,
}: {
  isDark: boolean;
  icon: React.ElementType;
  title: string;
  description: string;
  children: React.ReactNode;
}) => (
  <div className={cardClass(isDark)}>
    <div className="flex items-center gap-2 mb-1">
      <div className={`p-1.5 rounded-md ${isDark ? "bg-slate-800 text-indigo-400" : "bg-indigo-50 text-indigo-600"}`}>
        <Icon className="w-4 h-4" />
      </div>
      <h3 className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{title}</h3>
    </div>
    <p className={`text-xs ml-9 mb-4 ${isDark ? "text-slate-500" : "text-gray-500"}`}>{description}</p>
    <div className="space-y-4 ml-1">{children}</div>
  </div>
);

export const NumberInputRow = ({
  label,
  value,
  onChange,
  isDark,
  min = 0,
  helperText,
  formatValue = true,
  onAutoSave,
}: {
  label: string;
  value: number;
  onChange: (val: number) => void;
  isDark: boolean;
  min?: number;
  helperText?: React.ReactNode;
  formatValue?: boolean;
  onAutoSave?: () => void;
}) => (
  <div className="w-full">
    <label className={labelClass(isDark)}>{label}</label>
    <div className="relative">
      <input
        type="number"
        min={min}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        onBlur={() => onAutoSave?.()}
        className={inputClass(isDark)}
      />
      {formatValue ? (
        <div
          className={`
            absolute right-3 top-1/2 -translate-y-1/2 text-xs font-mono
            pointer-events-none transition-opacity duration-200
            ${isDark ? "text-slate-600" : "text-gray-400"}
          `}
        >
          {formatDuration(value)}
        </div>
      ) : null}
    </div>
    {helperText && (
      <div className={`mt-1.5 text-[11px] leading-snug ${isDark ? "text-slate-500" : "text-gray-500"}`}>
        {helperText}
      </div>
    )}
  </div>
);

export const Chip = ({
  label,
  onRemove,
  isDark,
}: {
  label: string;
  onRemove?: () => void;
  isDark: boolean;
}) => (
  <span
    className={`inline-flex items-center gap-1 px-2 py-1 rounded-full text-[11px] border ${
      isDark ? "border-slate-700 bg-slate-900 text-slate-200" : "border-gray-200 bg-white text-gray-700"
    }`}
  >
    <span className="font-mono">{label}</span>
    {onRemove ? (
      <button
        type="button"
        onClick={onRemove}
        className={`ml-0.5 rounded-full w-4 h-4 flex items-center justify-center ${
          isDark ? "hover:bg-slate-800 text-slate-300" : "hover:bg-gray-100 text-gray-500"
        }`}
        aria-label={`Remove ${label}`}
      >
        ×
      </button>
    ) : null}
  </span>
);

export function clampInt(v: number, min: number, max: number) {
  const n = Number.isFinite(v) ? Math.trunc(v) : min;
  return Math.max(min, Math.min(max, n));
}

export function isValidId(id: string) {
  return /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(id);
}

export function nowId(prefix: string) {
  return `${prefix}_${Date.now().toString(36)}`;
}

export function defaultNotifyAction(): Extract<AutomationRuleAction, { kind: "notify" }> {
  return { kind: "notify", priority: "high", requires_ack: false, snippet_ref: null, message: "" };
}

export function defaultGroupStateAction(): Extract<AutomationRuleAction, { kind: "group_state" }> {
  return { kind: "group_state", state: "paused" };
}

export function defaultActorControlAction(): Extract<AutomationRuleAction, { kind: "actor_control" }> {
  return { kind: "actor_control", operation: "restart", targets: ["@all"] };
}

export function actionKind(action: AutomationRule["action"] | undefined): "notify" | "group_state" | "actor_control" {
  const kind = String(action?.kind || "notify").trim();
  if (kind === "group_state" || kind === "actor_control") return kind;
  return "notify";
}

export const GROUP_STATE_COPY: Record<"active" | "idle" | "paused" | "stopped", { label: string; hint: string }> = {
  active: { label: "Activate Group", hint: "Start runners if needed, then resume active automation." },
  idle: { label: "Set Idle", hint: "Keep sessions running but disable proactive automation." },
  paused: { label: "Pause Delivery", hint: "Pause automation and notification delivery." },
  stopped: { label: "Stop Group", hint: "Stop all actor runtimes for this group." },
};

export const ACTOR_OPERATION_COPY: Record<"start" | "stop" | "restart", { label: string; hint: string }> = {
  start: { label: "Start Runtimes", hint: "Start selected actor runtimes." },
  stop: { label: "Stop Runtimes", hint: "Stop selected actor runtimes." },
  restart: { label: "Restart Runtimes", hint: "Restart selected actor runtimes." },
};

export function localTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

export type SchedulePreset = "daily" | "weekly" | "monthly";

export const WEEKDAY_OPTIONS: Array<{ value: number; label: string }> = [
  { value: 1, label: "Mon" },
  { value: 2, label: "Tue" },
  { value: 3, label: "Wed" },
  { value: 4, label: "Thu" },
  { value: 5, label: "Fri" },
  { value: 6, label: "Sat" },
  { value: 0, label: "Sun" },
];

export const AUTOMATION_VAR_HELP: Record<string, { description: string; example: string }> = {
  interval_minutes: {
    description: "Minutes for interval schedules (0 when not interval based).",
    example: "15",
  },
  group_title: {
    description: "Current group name.",
    example: "Riichi Arena Ops",
  },
  actor_names: {
    description: "Comma-separated enabled member names.",
    example: "foreman, peer1, peer2",
  },
  scheduled_at: {
    description: "Planned send time in UTC (ISO).",
    example: "2026-02-10T12:00:00Z",
  },
};

export function parseCronToPreset(
  cronExpr: string
): { preset: SchedulePreset; hour: number; minute: number; weekday: number; dayOfMonth: number } {
  const raw = String(cronExpr || "").trim();
  const parts = raw.split(/\s+/).filter(Boolean);
  if (parts.length !== 5) {
    return { preset: "daily", hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  }
  const [mStr, hStr, dom, mon, dow] = parts;
  if (!/^\d+$/.test(mStr) || !/^\d+$/.test(hStr)) {
    return { preset: "daily", hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  }
  const minute = clampInt(Number(mStr), 0, 59);
  const hour = clampInt(Number(hStr), 0, 23);

  if (dom === "*" && mon === "*" && dow === "*") {
    return { preset: "daily", hour, minute, weekday: 1, dayOfMonth: 1 };
  }
  if (dom === "*" && mon === "*" && /^\d+$/.test(dow)) {
    const weekdayRaw = Number(dow);
    const weekday = weekdayRaw === 7 ? 0 : clampInt(weekdayRaw, 0, 6);
    return { preset: "weekly", hour, minute, weekday, dayOfMonth: 1 };
  }
  if (/^\d+$/.test(dom) && mon === "*" && dow === "*") {
    const dayOfMonth = clampInt(Number(dom), 1, 31);
    return { preset: "monthly", hour, minute, weekday: 1, dayOfMonth };
  }
  return { preset: "daily", hour, minute, weekday: 1, dayOfMonth: 1 };
}

export function buildCronFromPreset(args: {
  preset: SchedulePreset;
  hour: number;
  minute: number;
  weekday: number;
  dayOfMonth: number;
}): string {
  const hour = clampInt(args.hour, 0, 23);
  const minute = clampInt(args.minute, 0, 59);
  const weekday = clampInt(args.weekday, 0, 6);
  const dayOfMonth = clampInt(args.dayOfMonth, 1, 31);
  if (args.preset === "weekly") {
    return `${minute} ${hour} * * ${weekday}`;
  }
  if (args.preset === "monthly") {
    return `${minute} ${hour} ${dayOfMonth} * *`;
  }
  return `${minute} ${hour} * * *`;
}

export function isoToLocalDatetimeInput(iso: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (!Number.isFinite(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  const yyyy = d.getFullYear();
  const mm = pad(d.getMonth() + 1);
  const dd = pad(d.getDate());
  const hh = pad(d.getHours());
  const min = pad(d.getMinutes());
  return `${yyyy}-${mm}-${dd}T${hh}:${min}`;
}

export function localDatetimeInputToIso(input: string): string {
  const raw = String(input || "").trim();
  if (!raw) return "";
  const d = new Date(raw);
  if (!Number.isFinite(d.getTime())) return "";
  return d.toISOString();
}

export function formatTimeInput(hour: number, minute: number): string {
  const h = String(clampInt(hour, 0, 23)).padStart(2, "0");
  const m = String(clampInt(minute, 0, 59)).padStart(2, "0");
  return `${h}:${m}`;
}

export function parseTimeInput(input: string): { hour: number; minute: number } {
  const raw = String(input || "").trim();
  const m = /^(\d{1,2}):(\d{1,2})$/.exec(raw);
  if (!m) return { hour: 9, minute: 0 };
  return {
    hour: clampInt(Number(m[1]), 0, 23),
    minute: clampInt(Number(m[2]), 0, 59),
  };
}
