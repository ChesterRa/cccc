import React from "react";
import { useTranslation } from "react-i18next";

import type { AutomationRule, AutomationRuleStatus } from "../../../types";
import {
  ACTOR_OPERATION_COPY,
  Chip,
  GROUP_STATE_COPY,
  WEEKDAY_OPTIONS,
  buildCronFromPreset,
  clampInt,
  defaultActorControlAction,
  defaultGroupStateAction,
  defaultNotifyAction,
  formatDuration,
  formatTimeInput,
  isoToLocalDatetimeInput,
  localDatetimeInputToIso,
  localTimeZone,
  parseCronToPreset,
  parseTimeInput,
} from "./automationUtils";
import type { SchedulePreset } from "./automationUtils";
import { inputClass, labelClass } from "./types";

interface AutomationRuleEditorModalProps {
  isDark: boolean;
  editingRule: AutomationRule | null;
  editingRuleStatus: AutomationRuleStatus;
  rulesErr: string;
  snippetIds: string[];
  actorTargetOptions: Array<{ value: string; label: string }>;
  oneShotModeByRule: Record<string, "after" | "exact">;
  oneShotAfterMinutesByRule: Record<string, number>;
  onRulePatch: (ruleId: string, patch: Partial<AutomationRule>) => void;
  onRuleRemove: (ruleId: string) => void;
  onSetEditingRuleId: (ruleId: string | null) => void;
  onSetRulesErr: (message: string) => void;
  onSetOneShotMode: (ruleId: string, mode: "after" | "exact") => void;
  onSetOneShotAfterMinutes: (ruleId: string, minutes: number) => void;
}

export function AutomationRuleEditorModal(props: AutomationRuleEditorModalProps) {
  const {
    isDark,
    editingRule,
    editingRuleStatus,
    rulesErr,
    snippetIds,
    actorTargetOptions,
    oneShotModeByRule,
    oneShotAfterMinutesByRule,
    onRulePatch,
    onRuleRemove,
    onSetEditingRuleId,
    onSetRulesErr,
    onSetOneShotMode,
    onSetOneShotAfterMinutes,
  } = props;

  const { t } = useTranslation("settings");

  if (!editingRule) return null;

  const ruleId = String(editingRule.id || "").trim();
  const ruleStatus = editingRuleStatus || {};
  const recipients = Array.isArray(editingRule.to) ? editingRule.to.map((x) => String(x || "").trim()).filter(Boolean) : [];
  const triggerKind = String(editingRule.trigger?.kind || "interval");
  const everySeconds = clampInt(
    Number(triggerKind === "interval" && editingRule.trigger && "every_seconds" in editingRule.trigger ? editingRule.trigger.every_seconds : 0),
    1,
    365 * 24 * 3600
  );
  const cronExpr = String(triggerKind === "cron" && editingRule.trigger && "cron" in editingRule.trigger ? editingRule.trigger.cron : "").trim();
  const atRaw = String(triggerKind === "at" && editingRule.trigger && "at" in editingRule.trigger ? editingRule.trigger.at : "").trim();
  const kind = String(editingRule.action?.kind || "notify").trim() as "notify" | "group_state" | "actor_control";
  const scheduleLockedToOneTime = kind !== "notify";
  const scheduleSelectValue = scheduleLockedToOneTime ? "at" : triggerKind;
  const activeTriggerKind = scheduleLockedToOneTime ? "at" : triggerKind;
  const operationalActionsEnabled = activeTriggerKind === "at";
  const snippetRef = String(kind === "notify" && editingRule.action && "snippet_ref" in editingRule.action ? editingRule.action.snippet_ref || "" : "").trim();
  const message = String(kind === "notify" && editingRule.action && "message" in editingRule.action ? editingRule.action.message || "" : "");
  const contentMode: "snippet" | "custom" = snippetRef ? "snippet" : "custom";
  const groupStateValue = String(
    kind === "group_state" && editingRule.action && "state" in editingRule.action ? editingRule.action.state || "paused" : "paused"
  );
  const actorOperation = String(
    kind === "actor_control" && editingRule.action && "operation" in editingRule.action ? editingRule.action.operation || "restart" : "restart"
  );
  const actorTargets = Array.isArray(kind === "actor_control" && editingRule.action && "targets" in editingRule.action ? editingRule.action.targets : [])
    ? (editingRule.action as { targets?: string[] }).targets?.map((x) => String(x || "").trim()).filter(Boolean) || []
    : [];
  const notifyAction =
    kind === "notify" && editingRule.action && editingRule.action.kind === "notify" ? editingRule.action : defaultNotifyAction();
  const enabled = editingRule.enabled !== false;
  const scope = String(editingRule.scope || "group") === "personal" ? "personal" : "group";
  const ownerActorId = String(editingRule.owner_actor_id || "").trim();
  const localTz = localTimeZone();
  const schedule = parseCronToPreset(cronExpr);
  const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
  const atInput = isoToLocalDatetimeInput(atRaw);
  const oneShotMode = oneShotModeByRule[ruleId] || "exact";
  const oneShotAfterMinutes = clampInt(oneShotAfterMinutesByRule[ruleId] ?? 30, 1, 7 * 24 * 60);

  return (
    <div
      className="fixed inset-0 z-[1000]"
      role="dialog"
      aria-modal="true"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onSetEditingRuleId(null);
      }}
    >
      <div className="absolute inset-0 bg-black/50" />
      <div
        className={`absolute inset-2 sm:inset-auto sm:left-1/2 sm:top-1/2 sm:w-[min(840px,calc(100vw-20px))] sm:h-[min(78vh,760px)] sm:-translate-x-1/2 sm:-translate-y-1/2 rounded-xl sm:rounded-2xl border ${
          isDark ? "border-slate-800 bg-slate-950" : "border-gray-200 bg-white"
        } shadow-2xl flex flex-col overflow-hidden`}
      >
        <div className={`px-4 py-3 border-b ${isDark ? "border-slate-800" : "border-gray-200"} flex items-start gap-3`}>
          <div className="min-w-0">
            <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
              {t("ruleEditor.editRule")} <span className="font-mono">{ruleId || t("ruleEditor.unnamed")}</span>
            </div>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-400" : "text-gray-600"}`}>
              {t("ruleEditor.last")} {ruleStatus.last_fired_at || "—"} • {t("ruleEditor.next")} {ruleStatus.next_fire_at || "—"}{" "}
              {ruleStatus.completed ? `• ${t("ruleEditor.completed")} ${ruleStatus.completed_at || ruleStatus.last_fired_at || "—"}` : ""}{" "}
              {ruleStatus.last_error ? `• ${t("ruleEditor.error")} ${ruleStatus.last_error_at || "—"}` : ""}
            </div>
            <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {t("automation.draftHint")}
            </div>
          </div>

          <div className="ml-auto flex items-center gap-2 flex-wrap justify-end">
            <label className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"} flex items-center gap-2`}>
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => onRulePatch(ruleId, { enabled: e.target.checked })}
              />
              on
            </label>
            <button
              type="button"
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              }`}
              onClick={() => {
                onRuleRemove(ruleId);
                onSetEditingRuleId(null);
              }}
            >
              {t("common:delete")}
            </button>
            <button
              type="button"
              className={`px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
              }`}
              onClick={() => onSetEditingRuleId(null)}
            >
              {t("common:close")}
            </button>
          </div>
        </div>

        {rulesErr ? <div className={`px-4 pt-3 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{rulesErr}</div> : null}
        {ruleStatus.last_error ? <div className={`px-4 pt-1 text-xs ${isDark ? "text-rose-300" : "text-rose-600"}`}>{ruleStatus.last_error}</div> : null}

        <div className="p-3 sm:p-4 flex-1 overflow-auto space-y-3">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div>
              <label className={labelClass(isDark)}>{t("ruleEditor.ruleName")}</label>
              <input
                value={ruleId}
                onChange={(e) => {
                  const nextId = e.target.value;
                  onRulePatch(ruleId, { id: nextId });
                  if (nextId.trim()) onSetEditingRuleId(nextId.trim());
                }}
                className={`${inputClass(isDark)} font-mono`}
                placeholder="daily_checkin"
                spellCheck={false}
              />
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("ruleEditor.ruleNameHint")}
              </div>
            </div>
            <div>
              <label className={labelClass(isDark)}>{t("ruleEditor.scheduleType")}</label>
              <select
                value={scheduleSelectValue}
                disabled={scheduleLockedToOneTime}
                onChange={(e) => {
                  const nextKind = String(e.target.value || "interval");
                  if (nextKind === "cron") {
                    const presetCron = buildCronFromPreset({
                      preset: schedule.preset,
                      hour: schedule.hour,
                      minute: schedule.minute,
                      weekday: schedule.weekday,
                      dayOfMonth: schedule.dayOfMonth,
                    });
                    onRulePatch(ruleId, {
                      trigger: {
                        kind: "cron",
                        cron: cronExpr || presetCron,
                        timezone: localTz,
                      },
                    });
                    return;
                  }
                  if (nextKind === "at") {
                    onSetOneShotMode(ruleId, "after");
                    const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                    onRulePatch(ruleId, { trigger: { kind: "at", at: defaultAt } });
                    return;
                  }
                  onRulePatch(ruleId, { trigger: { kind: "interval", every_seconds: everySeconds } });
                }}
                className={inputClass(isDark)}
              >
                {kind === "notify" ? <option value="interval">{t("ruleEditor.intervalSchedule")}</option> : null}
                {kind === "notify" ? <option value="cron">{t("ruleEditor.recurringSchedule")}</option> : null}
                <option value="at">{t("ruleEditor.oneTimeSchedule")}</option>
              </select>
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {scheduleLockedToOneTime
                  ? t("ruleEditor.oneTimeOnly")
                  : activeTriggerKind === "interval"
                    ? t("ruleEditor.intervalHint")
                    : activeTriggerKind === "cron"
                      ? t("ruleEditor.recurringHint")
                      : t("ruleEditor.oneTimeHint")}
              </div>
            </div>
          </div>

          {scope === "personal" ? (
            <div className={`text-[11px] ${isDark ? "text-amber-300" : "text-amber-700"}`}>
              {t("ruleEditor.personalRule", { owner: ownerActorId || "unknown" })}
            </div>
          ) : null}

          {activeTriggerKind === "interval" ? (
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.repeatEvery")}</label>
                <input
                  type="number"
                  min={1}
                  value={Math.max(1, Math.round(everySeconds / 60))}
                  onChange={(e) =>
                    onRulePatch(ruleId, {
                      trigger: {
                        kind: "interval",
                        every_seconds: Math.max(1, Number(e.target.value || 1)) * 60,
                      },
                    })
                  }
                  className={inputClass(isDark)}
                />
              </div>
              <div className={`self-end text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("ruleEditor.currentCadence", { duration: formatDuration(everySeconds) })}
              </div>
            </div>
          ) : null}

          {activeTriggerKind === "cron" ? (
            <div className="space-y-3">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.pattern")}</label>
                  <select
                    value={schedule.preset}
                    onChange={(e) => {
                      const preset = String(e.target.value || "daily") as SchedulePreset;
                      const nextCron = buildCronFromPreset({
                        preset,
                        hour: schedule.hour,
                        minute: schedule.minute,
                        weekday: schedule.weekday,
                        dayOfMonth: schedule.dayOfMonth,
                      });
                      onRulePatch(ruleId, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                    }}
                    className={inputClass(isDark)}
                  >
                    <option value="daily">{t("ruleEditor.daily")}</option>
                    <option value="weekly">{t("ruleEditor.weekly")}</option>
                    <option value="monthly">{t("ruleEditor.monthly")}</option>
                  </select>
                </div>
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.time")}</label>
                  <input
                    type="time"
                    value={scheduleTime}
                    onChange={(e) => {
                      const parsed = parseTimeInput(e.target.value);
                      const nextCron = buildCronFromPreset({
                        preset: schedule.preset,
                        hour: parsed.hour,
                        minute: parsed.minute,
                        weekday: schedule.weekday,
                        dayOfMonth: schedule.dayOfMonth,
                      });
                      onRulePatch(ruleId, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                    }}
                    className={inputClass(isDark)}
                  />
                </div>
              </div>
              {schedule.preset === "weekly" ? (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.weekday")}</label>
                  <select
                    value={String(schedule.weekday)}
                    onChange={(e) => {
                      const nextCron = buildCronFromPreset({
                        preset: "weekly",
                        hour: schedule.hour,
                        minute: schedule.minute,
                        weekday: Number(e.target.value || 1),
                        dayOfMonth: schedule.dayOfMonth,
                      });
                      onRulePatch(ruleId, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                    }}
                    className={inputClass(isDark)}
                  >
                    {WEEKDAY_OPTIONS.map((day) => (
                      <option key={day.value} value={String(day.value)}>
                        {day.label}
                      </option>
                    ))}
                  </select>
                </div>
              ) : null}
              {schedule.preset === "monthly" ? (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.dayOfMonth")}</label>
                  <input
                    type="number"
                    min={1}
                    max={31}
                    value={schedule.dayOfMonth}
                    onChange={(e) => {
                      const nextCron = buildCronFromPreset({
                        preset: "monthly",
                        hour: schedule.hour,
                        minute: schedule.minute,
                        weekday: schedule.weekday,
                        dayOfMonth: Number(e.target.value || 1),
                      });
                      onRulePatch(ruleId, { trigger: { kind: "cron", cron: nextCron, timezone: localTz } });
                    }}
                    className={inputClass(isDark)}
                  />
                </div>
              ) : null}
            </div>
          ) : null}

          {activeTriggerKind === "at" ? (
            <div className="space-y-3">
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.oneTimeMode")}</label>
                <select
                  value={oneShotMode}
                  onChange={(e) => onSetOneShotMode(ruleId, String(e.target.value || "after") as "after" | "exact")}
                  className={inputClass(isDark)}
                >
                  <option value="after">{t("ruleEditor.afterCountdown")}</option>
                  <option value="exact">{t("ruleEditor.exactTime")}</option>
                </select>
              </div>

              {oneShotMode === "after" ? (
                <div className="space-y-2">
                  <div className="flex flex-wrap gap-2">
                    {[5, 10, 30, 60, 120].map((mins) => (
                      <button
                        key={mins}
                        type="button"
                        className={`px-2.5 py-1.5 rounded-lg text-xs transition-colors ${
                          isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-200"
                        }`}
                        onClick={() => onSetOneShotAfterMinutes(ruleId, mins)}
                      >
                        {mins >= 60 ? `${Math.round(mins / 60)}h` : `${mins}m`}
                      </button>
                    ))}
                  </div>
                  <div>
                    <label className={labelClass(isDark)}>{t("ruleEditor.remindMeIn")}</label>
                    <input
                      type="number"
                      min={1}
                      max={10080}
                      value={oneShotAfterMinutes}
                      onChange={(e) => {
                        const minutes = clampInt(Number(e.target.value || 1), 1, 7 * 24 * 60);
                        onSetOneShotAfterMinutes(ruleId, minutes);
                      }}
                      className={inputClass(isDark)}
                    />
                  </div>
                </div>
              ) : (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.dateTime")}</label>
                  <input
                    type="datetime-local"
                    value={atInput}
                    onChange={(e) => onRulePatch(ruleId, { trigger: { kind: "at", at: localDatetimeInputToIso(e.target.value) } })}
                    className={inputClass(isDark)}
                  />
                </div>
              )}

              <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("automation.savedSendTime")} <span className="font-mono break-all">{atRaw || "—"}</span>
              </div>
            </div>
          ) : null}

          <div>
            <label className={labelClass(isDark)}>{t("ruleEditor.action")}</label>
            <select
              value={kind}
              onChange={(e) => {
                const next = String(e.target.value || "notify");
                if (next !== "notify" && !operationalActionsEnabled) {
                  onSetRulesErr(t("automation.operationalActionsHint"));
                  return;
                }
                onSetRulesErr("");
                if (next === "group_state") {
                  const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                  onSetOneShotMode(ruleId, "after");
                  onRulePatch(ruleId, { action: defaultGroupStateAction(), trigger: { kind: "at", at: defaultAt } });
                  return;
                }
                if (next === "actor_control") {
                  const defaultAt = atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString();
                  onSetOneShotMode(ruleId, "after");
                  onRulePatch(ruleId, { action: defaultActorControlAction(), trigger: { kind: "at", at: defaultAt } });
                  return;
                }
                onRulePatch(ruleId, { action: defaultNotifyAction(), to: recipients.length > 0 ? recipients : ["@foreman"] });
              }}
              className={inputClass(isDark)}
            >
              <option value="notify">{t("ruleEditor.sendReminder")}</option>
              <option value="group_state" disabled={!operationalActionsEnabled}>
                {t("ruleEditor.setGroupStatus")}{operationalActionsEnabled ? "" : t("automation.oneTimeOnlySuffix")}
              </option>
              <option value="actor_control" disabled={!operationalActionsEnabled}>
                {t("ruleEditor.controlActorRuntimes")}{operationalActionsEnabled ? "" : t("automation.oneTimeOnlySuffix")}
              </option>
            </select>
            {!operationalActionsEnabled ? (
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("automation.operationalActionsOnly")}
              </div>
            ) : null}
          </div>

          {kind === "notify" ? (
            <>
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.sendTo")}</label>
                <div className="flex flex-wrap gap-2">
                  {recipients.map((token) => (
                    <Chip
                      key={token}
                      label={token}
                      isDark={isDark}
                      onRemove={() => onRulePatch(ruleId, { to: recipients.filter((x) => x !== token) })}
                    />
                  ))}
                  <select
                    value=""
                    onChange={(e) => {
                      const value = String(e.target.value || "").trim();
                      if (!value) return;
                      if (!recipients.includes(value)) onRulePatch(ruleId, { to: [...recipients, value] });
                    }}
                    className={`px-3 py-2 rounded-lg text-sm min-h-[44px] ${
                      isDark ? "bg-slate-900 text-slate-200 border border-slate-800" : "bg-white text-gray-800 border border-gray-200"
                    }`}
                  >
                    <option value="">{t("automation.addRecipient")}</option>
                    {actorTargetOptions.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.notificationSource")}</label>
                <select
                  value={contentMode}
                  onChange={(e) => {
                    const nextMode = String(e.target.value || "custom");
                    if (nextMode === "snippet") {
                      if (snippetIds.length === 0) {
                        onSetRulesErr(t("automation.createSnippetFirst"));
                        return;
                      }
                      onSetRulesErr("");
                      const nextSnippetRef = snippetRef || snippetIds[0] || "";
                      onRulePatch(ruleId, {
                        action: { ...notifyAction, snippet_ref: nextSnippetRef || null },
                      });
                      return;
                    }
                    onSetRulesErr("");
                    onRulePatch(ruleId, { action: { ...notifyAction, snippet_ref: null } });
                  }}
                  className={inputClass(isDark)}
                >
                  <option value="snippet">{t("ruleEditor.messageSnippet")}</option>
                  <option value="custom">{t("ruleEditor.typeText")}</option>
                </select>
              </div>

              {contentMode === "snippet" ? (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.selectSnippet")}</label>
                  <select
                    value={snippetRef}
                    onChange={(e) =>
                      onRulePatch(ruleId, { action: { ...notifyAction, snippet_ref: e.target.value || null } })
                    }
                    className={`${inputClass(isDark)} font-mono`}
                  >
                    <option value="">(select snippet)</option>
                    {snippetIds.map((snippetId) => (
                      <option key={snippetId} value={snippetId}>
                        {snippetId}
                      </option>
                    ))}
                  </select>
                  {snippetIds.length === 0 ? (
                    <div className={`mt-1 text-[11px] ${isDark ? "text-amber-300" : "text-amber-700"}`}>
                      {t("automation.noSnippetsYet")}
                    </div>
                  ) : null}
                </div>
              ) : (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.messageLabel")}</label>
                  <textarea
                    value={message}
                    onChange={(e) => onRulePatch(ruleId, { action: { ...notifyAction, message: e.target.value } })}
                    className={`${inputClass(isDark)} font-mono text-[12px]`}
                    style={{ minHeight: 140 }}
                    placeholder={t("automation.messagePlaceholder")}
                    spellCheck={false}
                  />
                </div>
              )}
            </>
          ) : null}

          {kind === "group_state" ? (
            <div>
              <label className={labelClass(isDark)}>{t("ruleEditor.groupStatusTarget")}</label>
              <select
                value={groupStateValue}
                onChange={(e) =>
                  onRulePatch(ruleId, {
                    action: { kind: "group_state", state: String(e.target.value || "paused") as "active" | "idle" | "paused" | "stopped" },
                  })
                }
                className={inputClass(isDark)}
              >
                <option value="active">{GROUP_STATE_COPY.active.label}</option>
                <option value="idle">{GROUP_STATE_COPY.idle.label}</option>
                <option value="paused">{GROUP_STATE_COPY.paused.label}</option>
                <option value="stopped">{GROUP_STATE_COPY.stopped.label}</option>
              </select>
              <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {GROUP_STATE_COPY[(groupStateValue as "active" | "idle" | "paused" | "stopped") || "paused"].hint}
              </div>
            </div>
          ) : null}

          {kind === "actor_control" ? (
            <div className="space-y-3">
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.runtimeOperation")}</label>
                <select
                  value={actorOperation}
                  onChange={(e) =>
                    onRulePatch(ruleId, {
                      action: {
                        kind: "actor_control",
                        operation: String(e.target.value || "restart") as "start" | "stop" | "restart",
                        targets: actorTargets.length > 0 ? actorTargets : ["@all"],
                      },
                    })
                  }
                  className={inputClass(isDark)}
                >
                  <option value="start">{ACTOR_OPERATION_COPY.start.label}</option>
                  <option value="stop">{ACTOR_OPERATION_COPY.stop.label}</option>
                  <option value="restart">{ACTOR_OPERATION_COPY.restart.label}</option>
                </select>
                <div className={`mt-1 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                  {ACTOR_OPERATION_COPY[(actorOperation as "start" | "stop" | "restart") || "restart"].hint}
                </div>
              </div>
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.targetActors")}</label>
                <div className="flex flex-wrap gap-2">
                  {actorTargets.map((token) => (
                    <Chip
                      key={token}
                      label={token}
                      isDark={isDark}
                      onRemove={() =>
                        onRulePatch(ruleId, {
                          action: {
                            kind: "actor_control",
                            operation: actorOperation as "start" | "stop" | "restart",
                            targets: actorTargets.filter((x) => x !== token),
                          },
                        })
                      }
                    />
                  ))}
                  <select
                    value=""
                    onChange={(e) => {
                      const value = String(e.target.value || "").trim();
                      if (!value) return;
                      if (actorTargets.includes(value)) return;
                      onRulePatch(ruleId, {
                        action: {
                          kind: "actor_control",
                          operation: actorOperation as "start" | "stop" | "restart",
                          targets: [...actorTargets, value],
                        },
                      });
                    }}
                    className={`px-3 py-2 rounded-lg text-sm min-h-[44px] ${
                      isDark ? "bg-slate-900 text-slate-200 border border-slate-800" : "bg-white text-gray-800 border border-gray-200"
                    }`}
                  >
                    <option value="">{t("automation.addTarget")}</option>
                    {actorTargetOptions.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
