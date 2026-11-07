/* eslint-disable react-refresh/only-export-components -- utility + component mixed file */
import React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleAction, AutomationRuleStatus } from "../../../types";
import { SparklesIcon } from "../../Icons";
import {
  settingsWorkspaceShellClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceBodyClass,
} from "./types";

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
  <section className={settingsWorkspaceShellClass(isDark)}>
    <div className={settingsWorkspaceHeaderClass(isDark)}>
      <div>
        <h3 className="flex items-center gap-2 text-sm font-semibold text-[var(--color-text-primary)]">
          <Icon className="h-4 w-4 shrink-0 text-[var(--color-text-secondary)]" />
          {title}
        </h3>
        <p className="mt-1 text-xs text-[var(--color-text-muted)]">{description}</p>
      </div>
    </div>
    <div className={settingsWorkspaceBodyClass}>{children}</div>
  </section>
);

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
    <span className="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
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
  return { kind: "notify", priority: "high", snippet_ref: null, message: "" };
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

// Minutes are enough for a schedule; seconds would only add noise.
export const AUTOMATION_TIME_FORMAT: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  timeZoneName: "short",
};

export function formatAutomationTime(value: string | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, AUTOMATION_TIME_FORMAT);
}

// The daemon reports why a rule will not fire from its own Group state; the
// Web's "idle" only means no member is working, so it is not used here.
export function formatAutomationNext(status: AutomationRuleStatus, t: TFunction): string {
  if (status.suppressed_reason === "group_paused") return t("layout:statusPaused");
  if (status.suppressed_reason === "group_stopped") return t("layout:statusStopped");
  if (status.suppressed_reason === "idle_standup") return t("ruleList.idleStandup");
  return status.next_fire_at ? formatAutomationTime(status.next_fire_at) : "—";
}

// Only a notification that reached the ledger counts; older state has no such record.
export function formatAutomationLastRun(status: AutomationRuleStatus, t: TFunction): string {
  if (!status.last_run_at) return "";
  const time = formatAutomationTime(status.last_run_at);
  return status.last_run_kind === "manual" ? t("ruleList.lastRunManual", { time }) : time;
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

// "custom" is a saved expression the daily, weekly and monthly patterns cannot
// show; the editor keeps it as written until another pattern is chosen.
export type SchedulePreset = "daily" | "weekly" | "monthly" | "custom";

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
  if (!raw) return { preset: "daily", hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  const custom = { preset: "custom" as const, hour: 9, minute: 0, weekday: 1, dayOfMonth: 1 };
  const parts = raw.split(/\s+/).filter(Boolean);
  if (parts.length !== 5) return custom;
  const [mStr, hStr, dom, mon, dow] = parts;
  if (!/^\d+$/.test(mStr) || !/^\d+$/.test(hStr)) return custom;
  // An out-of-range value stays as written rather than being clamped into another plan.
  const minute = Number(mStr);
  const hour = Number(hStr);
  if (minute > 59 || hour > 23) return custom;

  if (dom === "*" && mon === "*" && dow === "*") {
    return { preset: "daily", hour, minute, weekday: 1, dayOfMonth: 1 };
  }
  if (dom === "*" && mon === "*" && /^\d+$/.test(dow) && Number(dow) <= 7) {
    const weekdayRaw = Number(dow);
    const weekday = weekdayRaw === 7 ? 0 : weekdayRaw;
    return { preset: "weekly", hour, minute, weekday, dayOfMonth: 1 };
  }
  if (/^\d+$/.test(dom) && mon === "*" && dow === "*" && Number(dom) >= 1 && Number(dom) <= 31) {
    const dayOfMonth = Number(dom);
    return { preset: "monthly", hour, minute, weekday: 1, dayOfMonth };
  }
  return { ...custom, hour, minute };
}

// The daemon reads a cron rule without a saved time zone as UTC.
export function cronTimeZone(trigger: unknown): string {
  const zone =
    trigger && typeof trigger === "object" && "timezone" in trigger
      ? String((trigger as { timezone?: unknown }).timezone || "").trim()
      : "";
  return zone || "UTC";
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
