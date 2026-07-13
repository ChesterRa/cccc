/* eslint-disable react-refresh/only-export-components -- utility + component mixed file */
import React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleAction } from "../../../types";
import { BellIcon as AppBellIcon, SparklesIcon } from "../../Icons";
import { cardClass, inputClass, labelClass } from "./types";

export const BellIcon = ({ className }: { className?: string }) => (
  <AppBellIcon className={className} />
);
export const SparkIcon = ({ className }: { className?: string }) => (
  <SparklesIcon className={className} />
);

export const formatDuration = (secondsRaw: number, t?: TFunction): string => {
  const seconds = Number.isFinite(secondsRaw) ? Math.max(0, Math.trunc(secondsRaw)) : 0;
  if (seconds <= 0) return t ? t("automation.durationOff") : "Off";
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
  isDark?: boolean;
  icon: React.ElementType;
  title: string;
  description: string;
  children: React.ReactNode;
}) => (
  <div className={cardClass(isDark)}>
    <div className="flex items-center gap-2 mb-1">
      <div className="rounded-md border border-black/8 bg-[rgb(245,245,245)] p-1.5 text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
        <Icon className="w-4 h-4" />
      </div>
      <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</h3>
    </div>
    <p className="text-xs ml-9 mb-4 text-[var(--color-text-muted)]">{description}</p>
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
  isDark?: boolean;
  min?: number;
  helperText?: React.ReactNode;
  formatValue?: boolean;
  onAutoSave?: () => void;
}) => {
  const { t } = useTranslation("settings");
  return (
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
          <div className="absolute right-3 top-1/2 -translate-y-1/2 text-xs font-mono pointer-events-none transition-opacity duration-200 text-[var(--color-text-muted)]">
            {formatDuration(value, t)}
          </div>
        ) : null}
      </div>
      {helperText && (
        <div className="mt-1.5 text-[11px] leading-snug text-[var(--color-text-muted)]">
          {helperText}
        </div>
      )}
    </div>
  );
};

export const Chip = ({
  label,
  onRemove,
  isDark: _isDark,
}: {
  label: string;
  onRemove?: () => void;
  isDark?: boolean;
}) => {
  const { t } = useTranslation();
  return (
    <span className="inline-flex items-center gap-1 px-2 py-1 rounded-full text-[11px] border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
      <span className="font-mono">{label}</span>
      {onRemove ? (
        <button
          type="button"
          onClick={onRemove}
          className="ml-0.5 rounded-full w-4 h-4 flex items-center justify-center hover:bg-[var(--glass-tab-bg-hover)] text-[var(--color-text-tertiary)]"
          aria-label={`${t("common:remove")} ${label}`}
        >
          ×
        </button>
      ) : null}
    </span>
  );
};

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

export function defaultActorControlAction(): Extract<
  AutomationRuleAction,
  { kind: "actor_control" }
> {
  return { kind: "actor_control", operation: "restart", targets: ["@all"] };
}

export function actionKind(
  action: AutomationRule["action"] | undefined,
): "notify" | "group_state" | "actor_control" {
  const kind = String(action?.kind || "notify").trim();
  if (kind === "group_state" || kind === "actor_control") return kind;
  return "notify";
}

export function getGroupStateCopy(
  t: TFunction,
): Record<"active" | "idle" | "paused" | "stopped", { label: string; hint: string }> {
  return {
    active: {
      label: t("automation.groupStateActiveLabel"),
      hint: t("automation.groupStateActiveHint"),
    },
    idle: { label: t("automation.groupStateIdleLabel"), hint: t("automation.groupStateIdleHint") },
    paused: {
      label: t("automation.groupStatePausedLabel"),
      hint: t("automation.groupStatePausedHint"),
    },
    stopped: {
      label: t("automation.groupStateStoppedLabel"),
      hint: t("automation.groupStateStoppedHint"),
    },
  };
}

export function getActorOperationCopy(
  t: TFunction,
): Record<"start" | "stop" | "restart", { label: string; hint: string }> {
  return {
    start: { label: t("automation.actorOpStartLabel"), hint: t("automation.actorOpStartHint") },
    stop: { label: t("automation.actorOpStopLabel"), hint: t("automation.actorOpStopHint") },
    restart: {
      label: t("automation.actorOpRestartLabel"),
      hint: t("automation.actorOpRestartHint"),
    },
  };
}

export function localTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

export type SchedulePreset = "daily" | "weekly" | "monthly";

export function getWeekdayOptions(t: TFunction): Array<{ value: number; label: string }> {
  return [
    { value: 1, label: t("automation.weekdayMon") },
    { value: 2, label: t("automation.weekdayTue") },
    { value: 3, label: t("automation.weekdayWed") },
    { value: 4, label: t("automation.weekdayThu") },
    { value: 5, label: t("automation.weekdayFri") },
    { value: 6, label: t("automation.weekdaySat") },
    { value: 0, label: t("automation.weekdaySun") },
  ];
}

export function getAutomationVarHelp(
  t: TFunction,
): Record<string, { description: string; example: string }> {
  return {
    interval_minutes: { description: t("automation.varHelpIntervalMinutes"), example: "15" },
    group_title: { description: t("automation.varHelpGroupTitle"), example: "Riichi Arena Ops" },
    actor_names: {
      description: t("automation.varHelpActorNames"),
      example: "foreman, peer1, peer2",
    },
    scheduled_at: {
      description: t("automation.varHelpScheduledAt"),
      example: "2026-02-10T12:00:00Z",
    },
  };
}

export function parseCronToPreset(cronExpr: string): {
  preset: SchedulePreset;
  hour: number;
  minute: number;
  weekday: number;
  dayOfMonth: number;
} {
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
  return { hour: clampInt(Number(m[1]), 0, 23), minute: clampInt(Number(m[2]), 0, 59) };
}
