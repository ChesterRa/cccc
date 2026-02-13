import React from "react";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleStatus } from "../../../types";
import {
  ACTOR_OPERATION_COPY,
  GROUP_STATE_COPY,
  WEEKDAY_OPTIONS,
  actionKind,
  clampInt,
  formatTimeInput,
  isoToLocalDatetimeInput,
  parseCronToPreset,
} from "./automationUtils";
import { cardClass } from "./types";

interface AutomationRuleListProps {
  isDark: boolean;
  visibleRules: AutomationRule[];
  status: Record<string, AutomationRuleStatus>;
  rulesBusy: boolean;
  showCompletedRules: boolean;
  completedOneTimeRuleIds: string[];
  onToggleShowCompleted: (value: boolean) => void;
  onClearCompleted: () => void;
  onToggleRuleEnabled: (ruleId: string, enabled: boolean) => void;
  onEditRule: (ruleId: string) => void;
  onDeleteRule: (ruleId: string) => void;
}

export function AutomationRuleList({
  isDark,
  visibleRules,
  status,
  rulesBusy,
  showCompletedRules,
  completedOneTimeRuleIds,
  onToggleShowCompleted,
  onClearCompleted,
  onToggleRuleEnabled,
  onEditRule,
  onDeleteRule,
}: AutomationRuleListProps) {
  const { t } = useTranslation("settings");
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-end gap-2 flex-wrap">
        <label
          className={`inline-flex items-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] min-h-[32px] border ${
            isDark ? "border-slate-700 text-slate-300 bg-slate-900" : "border-gray-200 text-gray-700 bg-white"
          }`}
        >
          <input
            type="checkbox"
            checked={showCompletedRules}
            onChange={(e) => onToggleShowCompleted(Boolean(e.target.checked))}
            className="h-3 w-3"
          />
          {t("ruleList.showCompleted")}
        </label>
        <button
          type="button"
          className={`px-2 py-1.5 rounded-md text-[11px] min-h-[32px] font-medium transition-colors ${
            isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
          } disabled:opacity-50`}
          onClick={onClearCompleted}
          disabled={rulesBusy || completedOneTimeRuleIds.length === 0}
          title={t("ruleList.clearCompletedTitle")}
        >
          {t("ruleList.clearCompleted", { count: completedOneTimeRuleIds.length })}
        </button>
      </div>

      {visibleRules.length === 0 ? (
        <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-600"}`}>{t("ruleList.noRules")}</div>
      ) : null}

      {visibleRules.map((rule) => {
        const ruleId = String(rule.id || "").trim();
        const ruleStatus = status[ruleId] || {};
        const recipients = Array.isArray(rule.to) ? rule.to.map((x) => String(x || "").trim()).filter(Boolean) : [];
        const triggerKind = String(rule.trigger?.kind || "interval");
        const everySeconds = clampInt(
          Number(triggerKind === "interval" && rule.trigger && "every_seconds" in rule.trigger ? rule.trigger.every_seconds : 0),
          1,
          365 * 24 * 3600
        );
        const cronExpr = String(triggerKind === "cron" && rule.trigger && "cron" in rule.trigger ? rule.trigger.cron : "").trim();
        const atRaw = String(triggerKind === "at" && rule.trigger && "at" in rule.trigger ? rule.trigger.at : "").trim();
        const kind = actionKind(rule.action);
        const snippetRef = String(kind === "notify" && rule.action && "snippet_ref" in rule.action ? rule.action.snippet_ref || "" : "").trim();
        const message = String(kind === "notify" && rule.action && "message" in rule.action ? rule.action.message || "" : "").trim();
        const enabled = rule.enabled !== false;
        const nextFireAt = String(ruleStatus.next_fire_at || "").trim();
        const lastFireAt = String(ruleStatus.last_fired_at || "").trim();
        const schedule = parseCronToPreset(cronExpr);
        const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
        const weekdayLabel = WEEKDAY_OPTIONS.find((x) => x.value === schedule.weekday)?.label || String(schedule.weekday);
        const atLocal = atRaw ? isoToLocalDatetimeInput(atRaw).replace("T", " ") : "";

        let scheduleLabel = "Schedule not set";
        if (triggerKind === "interval") {
          scheduleLabel = `Every ${Math.max(1, Math.round(everySeconds / 60))} min`;
        } else if (triggerKind === "cron") {
          if (schedule.preset === "daily") scheduleLabel = `Daily ${scheduleTime}`;
          else if (schedule.preset === "weekly") scheduleLabel = `Weekly ${weekdayLabel} ${scheduleTime}`;
          else scheduleLabel = `Monthly day ${schedule.dayOfMonth} ${scheduleTime}`;
        } else if (triggerKind === "at") {
          scheduleLabel = atLocal ? `One-time ${atLocal}` : "One-time (time not set)";
        }

        let actionLabel = "Action not set";
        if (kind === "notify") {
          const contentLabel = snippetRef ? `Snippet: ${snippetRef}` : message ? "Typed message" : "Message not set";
          const recipientsLabel = recipients.length > 0 ? recipients.join(", ") : "(no recipients)";
          actionLabel = `Reminder -> ${recipientsLabel} • ${contentLabel}`;
        } else if (kind === "group_state") {
          const stateValue = String(rule.action && "state" in rule.action ? rule.action.state || "paused" : "paused");
          const normalizedState = (["active", "idle", "paused", "stopped"].includes(stateValue)
            ? stateValue
            : "paused") as "active" | "idle" | "paused" | "stopped";
          actionLabel = `Group status -> ${GROUP_STATE_COPY[normalizedState].label}`;
        } else if (kind === "actor_control") {
          const operation = String(rule.action && "operation" in rule.action ? rule.action.operation || "restart" : "restart");
          const targets = Array.isArray(rule.action && "targets" in rule.action ? rule.action.targets : [])
            ? (rule.action as { targets?: string[] }).targets?.map((x) => String(x || "").trim()).filter(Boolean) || []
            : [];
          const normalizedOperation = (["start", "stop", "restart"].includes(operation)
            ? operation
            : "restart") as "start" | "stop" | "restart";
          actionLabel = `${ACTOR_OPERATION_COPY[normalizedOperation].label} -> ${targets.length > 0 ? targets.join(", ") : "(no targets)"}`;
        }

        const hasError = Boolean(ruleStatus.last_error);
        const completed = Boolean(ruleStatus.completed) && triggerKind === "at";
        const completedAt = String(ruleStatus.completed_at || "").trim();

        return (
          <div key={ruleId} className={cardClass(isDark)}>
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>{ruleId || t("ruleList.rule")}</div>
                <div className={`mt-0.5 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {scheduleLabel} • {enabled ? t("ruleList.on") : t("ruleList.off")} {completed ? `• ${t("ruleList.completedLabel")}` : ""}
                </div>
              </div>

              <div className="flex items-center gap-2 shrink-0">
                <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"} flex items-center gap-2`}>
                  <input
                    type="checkbox"
                    checked={enabled}
                    onChange={(e) => onToggleRuleEnabled(ruleId, e.target.checked)}
                  />
                  on
                </label>
                <button
                  type="button"
                  className={`px-3 py-2 rounded-lg text-xs min-h-[36px] transition-colors ${
                    isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                  }`}
                  onClick={() => onEditRule(ruleId)}
                >
                  {t("common:edit")}
                </button>
                <button
                  type="button"
                  className={`px-3 py-2 rounded-lg text-xs min-h-[36px] transition-colors ${
                    isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                  }`}
                  onClick={() => onDeleteRule(ruleId)}
                  title={t("automation.deleteRuleTitle")}
                >
                  {t("common:delete")}
                </button>
              </div>
            </div>

            <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"} break-words`}>
              <span className="font-mono">{actionLabel}</span>
            </div>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              Last: {lastFireAt || "—"} • Next: {nextFireAt || "—"}
            </div>
            {completed ? (
              <div className={`mt-1 text-[11px] ${isDark ? "text-emerald-300" : "text-emerald-700"}`}>
                {t("ruleList.completedAt")} {completedAt || lastFireAt || "—"}
              </div>
            ) : null}
            {hasError ? (
              <div className={`mt-1 text-[11px] break-words ${isDark ? "text-rose-300" : "text-rose-600"}`}>{ruleStatus.last_error}</div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
