import React from "react";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleStatus } from "../../../types";
import {
  actionKind,
  clampInt,
  formatTimeInput,
  getActorOperationCopy,
  getGroupStateCopy,
  getWeekdayOptions,
  isoToLocalDatetimeInput,
  parseCronToPreset,
} from "./automationUtils";
import { cardClass, dangerButtonClass, secondaryButtonClass } from "./types";

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
  const actorOperationCopy = getActorOperationCopy(t);
  const groupStateCopy = getGroupStateCopy(t);
  const weekdayOptions = getWeekdayOptions(t);

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-end gap-2 flex-wrap">
        <label className="inline-flex items-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] min-h-[32px] border border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)] bg-[var(--glass-tab-bg)]">
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
          className={secondaryButtonClass("sm")}
          onClick={onClearCompleted}
          disabled={rulesBusy || completedOneTimeRuleIds.length === 0}
          title={t("ruleList.clearCompletedTitle")}
        >
          {t("ruleList.clearCompleted", { count: completedOneTimeRuleIds.length })}
        </button>
      </div>

      {visibleRules.length === 0 ? (
        <div className="text-sm text-[var(--color-text-tertiary)]">{t("ruleList.noRules")}</div>
      ) : null}

      {visibleRules.map((rule) => {
        const ruleId = String(rule.id || "").trim();
        const ruleStatus = status[ruleId] || {};
        const recipients = Array.isArray(rule.to)
          ? rule.to.map((x) => String(x || "").trim()).filter(Boolean)
          : [];
        const triggerKind = String(rule.trigger?.kind || "interval");
        const everySeconds = clampInt(
          Number(
            triggerKind === "interval" && rule.trigger && "every_seconds" in rule.trigger
              ? rule.trigger.every_seconds
              : 0,
          ),
          1,
          365 * 24 * 3600,
        );
        const cronExpr = String(
          triggerKind === "cron" && rule.trigger && "cron" in rule.trigger ? rule.trigger.cron : "",
        ).trim();
        const atRaw = String(
          triggerKind === "at" && rule.trigger && "at" in rule.trigger ? rule.trigger.at : "",
        ).trim();
        const kind = actionKind(rule.action);
        const snippetRef = String(
          kind === "notify" && rule.action && "snippet_ref" in rule.action
            ? rule.action.snippet_ref || ""
            : "",
        ).trim();
        const message = String(
          kind === "notify" && rule.action && "message" in rule.action
            ? rule.action.message || ""
            : "",
        ).trim();
        const enabled = rule.enabled !== false;
        const nextFireAt = String(ruleStatus.next_fire_at || "").trim();
        const lastFireAt = String(ruleStatus.last_fired_at || "").trim();
        const schedule = parseCronToPreset(cronExpr);
        const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
        const weekdayLabel =
          weekdayOptions.find((x) => x.value === schedule.weekday)?.label ||
          String(schedule.weekday);
        const atLocal = atRaw ? isoToLocalDatetimeInput(atRaw).replace("T", " ") : "";

        let scheduleLabel = t("ruleList.scheduleNotSet");
        if (triggerKind === "interval") {
          scheduleLabel = t("ruleList.scheduleEveryMinutes", {
            count: Math.max(1, Math.round(everySeconds / 60)),
          });
        } else if (triggerKind === "cron") {
          if (schedule.preset === "daily")
            scheduleLabel = t("ruleList.scheduleDailyAt", { time: scheduleTime });
          else if (schedule.preset === "weekly")
            scheduleLabel = t("ruleList.scheduleWeeklyAt", {
              weekday: weekdayLabel,
              time: scheduleTime,
            });
          else
            scheduleLabel = t("ruleList.scheduleMonthlyAt", {
              day: schedule.dayOfMonth,
              time: scheduleTime,
            });
        } else if (triggerKind === "at") {
          scheduleLabel = atLocal
            ? t("ruleList.scheduleOneTimeAt", { time: atLocal })
            : t("ruleList.scheduleOneTimeUnset");
        }

        let actionLabel = t("ruleList.actionNotSet");
        if (kind === "notify") {
          const contentLabel = snippetRef
            ? t("ruleList.snippetLabel", { id: snippetRef })
            : message
              ? t("ruleList.typedMessage")
              : t("ruleList.messageNotSet");
          const recipientsLabel =
            recipients.length > 0 ? recipients.join(", ") : t("ruleList.noRecipients");
          actionLabel = t("ruleList.reminderAction", {
            recipients: recipientsLabel,
            content: contentLabel,
          });
        } else if (kind === "group_state") {
          const stateValue = String(
            rule.action && "state" in rule.action ? rule.action.state || "paused" : "paused",
          );
          const normalizedState = (
            ["active", "idle", "paused", "stopped"].includes(stateValue) ? stateValue : "paused"
          ) as "active" | "idle" | "paused" | "stopped";
          actionLabel = t("ruleList.groupStatusAction", {
            label: groupStateCopy[normalizedState].label,
          });
        } else if (kind === "actor_control") {
          const operation = String(
            rule.action && "operation" in rule.action
              ? rule.action.operation || "restart"
              : "restart",
          );
          const targets = Array.isArray(
            rule.action && "targets" in rule.action ? rule.action.targets : [],
          )
            ? (rule.action as { targets?: string[] }).targets
                ?.map((x) => String(x || "").trim())
                .filter(Boolean) || []
            : [];
          const normalizedOperation = (
            ["start", "stop", "restart"].includes(operation) ? operation : "restart"
          ) as "start" | "stop" | "restart";
          actionLabel = t("ruleList.actorControlAction", {
            label: actorOperationCopy[normalizedOperation].label,
            targets: targets.length > 0 ? targets.join(", ") : t("ruleList.noTargets"),
          });
        }

        const hasError = Boolean(ruleStatus.last_error);
        const completed = Boolean(ruleStatus.completed) && triggerKind === "at";
        const completedAt = String(ruleStatus.completed_at || "").trim();

        return (
          <div key={ruleId} className={cardClass(isDark)}>
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {ruleId || t("ruleList.rule")}
                </div>
                <div className="mt-0.5 text-[11px] text-[var(--color-text-muted)]">
                  {scheduleLabel} • {enabled ? t("ruleList.on") : t("ruleList.off")}{" "}
                  {completed ? `• ${t("ruleList.completedLabel")}` : ""}
                </div>
              </div>

              <div className="flex items-center gap-2 shrink-0">
                <label className="text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={enabled}
                    onChange={(e) => onToggleRuleEnabled(ruleId, e.target.checked)}
                  />
                  {t("ruleList.on")}
                </label>
                <button
                  type="button"
                  className={secondaryButtonClass("sm")}
                  onClick={() => onEditRule(ruleId)}
                >
                  {t("common:edit")}
                </button>
                <button
                  type="button"
                  className={dangerButtonClass("sm")}
                  onClick={() => onDeleteRule(ruleId)}
                  title={t("automation.deleteRuleTitle")}
                >
                  {t("common:delete")}
                </button>
              </div>
            </div>

            <div className="mt-2 text-[11px] text-[var(--color-text-muted)] break-words">
              <span className="font-mono">{actionLabel}</span>
            </div>
            <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
              {t("ruleList.lastShort")}: {lastFireAt || "—"} • {t("ruleList.nextShort")}:{" "}
              {nextFireAt || "—"}
            </div>
            {completed ? (
              <div className="mt-1 text-[11px] text-emerald-700 dark:text-emerald-300">
                {t("ruleList.completedAt")} {completedAt || lastFireAt || "—"}
              </div>
            ) : null}
            {hasError ? (
              <div className="mt-1 text-[11px] break-words text-rose-600 dark:text-rose-300">
                {ruleStatus.last_error}
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
