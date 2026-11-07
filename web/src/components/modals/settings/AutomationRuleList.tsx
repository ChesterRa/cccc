import React from "react";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleStatus } from "../../../types";
import {
  actionKind,
  clampInt,
  cronTimeZone,
  formatAutomationLastRun,
  formatAutomationNext,
  formatTimeInput,
  getActorOperationCopy,
  getGroupStateCopy,
  getWeekdayOptions,
  isoToLocalDatetimeInput,
  localTimeZone,
  parseCronToPreset,
} from "./automationUtils";
import { cardClass, dangerButtonClass, secondaryButtonClass } from "./types";

interface AutomationRuleListProps {
  isDark: boolean;
  groupState?: string;
  visibleRules: AutomationRule[];
  status: Record<string, AutomationRuleStatus>;
  rulesBusy: boolean;
  showCompletedRules: boolean;
  completedRuleIds: string[];
  onToggleShowCompleted: (value: boolean) => void;
  onClearCompleted: () => void;
  onToggleRuleEnabled: (ruleId: string, enabled: boolean) => void;
  onEditRule: (ruleId: string) => void;
  /** Sends a saved notify rule's notification once, outside its schedule. */
  onRunRule?: (ruleId: string) => void;
  /** The outcome of the last run started from this list, shown on that rule. */
  runNotice?: { ruleId: string; text: string; tone: "ok" | "warning" } | null;
  onDeleteRule: (ruleId: string) => void;
}

export function AutomationRuleList({
  isDark,
  groupState,
  visibleRules,
  status,
  rulesBusy,
  showCompletedRules,
  completedRuleIds,
  onToggleShowCompleted,
  onClearCompleted,
  onToggleRuleEnabled,
  onEditRule,
  onRunRule,
  runNotice,
  onDeleteRule,
}: AutomationRuleListProps) {
  const { t } = useTranslation("settings");
  const actorOperationCopy = getActorOperationCopy(t);
  const groupStateCopy = getGroupStateCopy(t);
  const weekdayOptions = getWeekdayOptions(t);

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-end gap-2 flex-wrap">
        <label className="inline-flex items-center gap-1.5 px-2 py-1.5 rounded-md text-xs min-h-[32px] border border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)] bg-[var(--glass-tab-bg)]">
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
          disabled={rulesBusy || completedRuleIds.length === 0}
          title={t("ruleList.clearCompletedTitle")}
        >
          {t("ruleList.clearCompleted", { count: completedRuleIds.length })}
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
        const lastRun = formatAutomationLastRun(ruleStatus, t);
        const runBlocked = groupState === "paused" || groupState === "stopped";
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
          if (schedule.preset === "custom") scheduleLabel = cronExpr;
          else if (schedule.preset === "daily")
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
          // The hours above are in the rule's own zone, which may not be the browser's.
          const ruleZone = cronTimeZone(rule.trigger);
          if (ruleZone !== localTimeZone()) scheduleLabel = `${scheduleLabel} (${ruleZone})`;
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
        const completed = Boolean(ruleStatus.completed);
        const maxRuns =
          rule.trigger && rule.trigger.kind !== "at" ? rule.trigger.max_runs : undefined;

        return (
          <div key={ruleId} className={cardClass(isDark)}>
            {/* On a phone the controls wrap below the title instead of squeezing it. */}
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-[11rem] flex-1 break-words">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {ruleId || t("ruleList.rule")}
                </div>
                <div className="mt-0.5 text-xs text-[var(--color-text-muted)]">
                  {scheduleLabel} • {enabled ? t("ruleList.on") : t("ruleList.off")}{" "}
                  {completed ? `• ${t("ruleList.completedLabel")}` : ""}
                  {maxRuns !== undefined
                    ? ` • ${t("ruleList.runProgress", { done: ruleStatus.fired_count || 0, max: maxRuns })}`
                    : ""}
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <label className="text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={enabled}
                    disabled={completed && triggerKind !== "at"}
                    title={
                      completed && triggerKind !== "at"
                        ? t("ruleList.completedLimitHint")
                        : undefined
                    }
                    onChange={(e) => onToggleRuleEnabled(ruleId, e.target.checked)}
                  />
                  {t("ruleList.on")}
                </label>
                {kind === "notify" && onRunRule ? (
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    disabled={rulesBusy || runBlocked}
                    title={
                      runBlocked
                        ? t(
                            groupState === "paused"
                              ? "automation.runPaused"
                              : "automation.runStopped",
                          )
                        : t("ruleList.runNowHint")
                    }
                    onClick={() => onRunRule(ruleId)}
                  >
                    {t("ruleList.runNow")}
                  </button>
                ) : null}
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

            <div className="mt-2 text-xs text-[var(--color-text-muted)] break-words">
              <span className="font-mono">{actionLabel}</span>
            </div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">
              {lastRun ? `${t("ruleList.lastShort")}: ${lastRun} • ` : ""}
              {t("ruleList.nextShort")}: {formatAutomationNext(ruleStatus, t)}
            </div>
            {runNotice && runNotice.ruleId === ruleId ? (
              <div
                data-status-tone={runNotice.tone}
                className={
                  runNotice.tone === "ok"
                    ? "mt-1 text-xs text-emerald-700 dark:text-emerald-300"
                    : "mt-1 text-xs text-amber-700 dark:text-amber-300"
                }
              >
                {runNotice.text}
              </div>
            ) : null}
            {hasError ? (
              <div
                data-status-tone="error"
                className="mt-1 text-xs break-words text-rose-600 dark:text-rose-300"
              >
                {ruleStatus.last_error}
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
