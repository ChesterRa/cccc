import React from "react";
import { useTranslation } from "react-i18next";
import { BodyPortal } from "../../ui/BodyPortal";
import { GroupCombobox } from "../../GroupCombobox";
import { SelectCombobox } from "../../SelectCombobox";

import type { AutomationRule, AutomationRuleStatus } from "../../../types";
import {
  Chip,
  buildCronFromPreset,
  clampInt,
  defaultActorControlAction,
  defaultGroupStateAction,
  defaultNotifyAction,
  formatDuration,
  formatTimeInput,
  getActorOperationCopy,
  getGroupStateCopy,
  getWeekdayOptions,
  isoToLocalDatetimeInput,
  localDatetimeInputToIso,
  localTimeZone,
  parseCronToPreset,
  parseTimeInput,
} from "./automationUtils";
import type { SchedulePreset } from "./automationUtils";
import {
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsDialogBodyClass,
  settingsDialogFooterClass,
  settingsDialogHeaderClass,
  settingsDialogPanelClass,
} from "./types";

interface AutomationRuleEditorModalProps {
  isDark: boolean;
  ruleDraft: AutomationRule | null;
  ruleStatus: AutomationRuleStatus;
  isNewRule: boolean;
  errorMessage: string;
  saveBusy: boolean;
  snippetIds: string[];
  snippets: Record<string, string>;
  actorTargetOptions: Array<{ value: string; label: string }>;
  oneShotMode: "after" | "exact";
  oneShotAfterMinutes: number;
  onRuleChange: (next: AutomationRule | null) => void;
  onClose: () => void;
  onSetRulesErr: (message: string) => void;
  onSetOneShotMode: (mode: "after" | "exact") => void;
  onSetOneShotAfterMinutes: (minutes: number) => void;
  onSave: () => void | Promise<void>;
}

function buildDefaultOneShotAt(): string {
  return new Date(Date.now() + 30 * 60 * 1000).toISOString();
}

export function AutomationRuleEditorModal(props: AutomationRuleEditorModalProps) {
  const {
    isDark,
    ruleDraft,
    ruleStatus,
    isNewRule,
    errorMessage,
    saveBusy,
    snippetIds,
    snippets,
    actorTargetOptions,
    oneShotMode,
    oneShotAfterMinutes,
    onRuleChange,
    onClose,
    onSetRulesErr,
    onSetOneShotMode,
    onSetOneShotAfterMinutes,
    onSave,
  } = props;

  const { t } = useTranslation("settings");
  const actorOperationCopy = getActorOperationCopy(t);
  const groupStateCopy = getGroupStateCopy(t);
  const weekdayOptions = getWeekdayOptions(t);

  if (!ruleDraft) return null;

  const patchRule = (patch: Partial<AutomationRule>) => {
    onRuleChange({ ...ruleDraft, ...patch });
  };

  const ruleId = String(ruleDraft.id || "").trim();
  const status = ruleStatus || {};
  const recipients = Array.isArray(ruleDraft.to) ? ruleDraft.to.map((item) => String(item || "").trim()).filter(Boolean) : [];
  const triggerKind = String(ruleDraft.trigger?.kind || "interval");
  const everySeconds = clampInt(
    Number(triggerKind === "interval" && ruleDraft.trigger && "every_seconds" in ruleDraft.trigger ? ruleDraft.trigger.every_seconds : 0),
    1,
    365 * 24 * 3600
  );
  const cronExpr = String(triggerKind === "cron" && ruleDraft.trigger && "cron" in ruleDraft.trigger ? ruleDraft.trigger.cron : "").trim();
  const atRaw = String(triggerKind === "at" && ruleDraft.trigger && "at" in ruleDraft.trigger ? ruleDraft.trigger.at : "").trim();
  const kind = String(ruleDraft.action?.kind || "notify").trim() as "notify" | "group_state" | "actor_control";
  const scheduleLockedToOneTime = kind !== "notify";
  const scheduleSelectValue = scheduleLockedToOneTime ? "at" : triggerKind;
  const activeTriggerKind = scheduleLockedToOneTime ? "at" : triggerKind;
  const operationalActionsEnabled = activeTriggerKind === "at";
  const snippetRef = String(kind === "notify" && ruleDraft.action && "snippet_ref" in ruleDraft.action ? ruleDraft.action.snippet_ref || "" : "").trim();
  const selectedSnippetContent = snippetRef ? String(snippets[snippetRef] || "").trim() : "";
  const snippetItems = snippetIds.map((snippetId) => ({
    value: snippetId,
    label: snippetId,
    description: String(snippets[snippetId] || "").trim().split("\n")[0] || t("ruleEditor.snippetPreviewEmpty", { defaultValue: "This snippet is empty." }),
  }));
  const availableRecipientOptions = actorTargetOptions.filter((option) => !recipients.includes(option.value));
  const message = String(kind === "notify" && ruleDraft.action && "message" in ruleDraft.action ? ruleDraft.action.message || "" : "");
  const contentMode: "snippet" | "custom" = snippetRef ? "snippet" : "custom";
  const groupStateValue = String(
    kind === "group_state" && ruleDraft.action && "state" in ruleDraft.action ? ruleDraft.action.state || "paused" : "paused"
  );
  const actorOperation = String(
    kind === "actor_control" && ruleDraft.action && "operation" in ruleDraft.action ? ruleDraft.action.operation || "restart" : "restart"
  );
  const actorTargets = Array.isArray(kind === "actor_control" && ruleDraft.action && "targets" in ruleDraft.action ? ruleDraft.action.targets : [])
    ? (ruleDraft.action as { targets?: string[] }).targets?.map((item) => String(item || "").trim()).filter(Boolean) || []
    : [];
  const availableActorTargetOptions = actorTargetOptions.filter((option) => !actorTargets.includes(option.value));
  const notifyAction =
    kind === "notify" && ruleDraft.action && ruleDraft.action.kind === "notify" ? ruleDraft.action : defaultNotifyAction();
  const enabled = ruleDraft.enabled !== false;
  const scope = String(ruleDraft.scope || "group") === "personal" ? "personal" : "group";
  const ownerActorId = String(ruleDraft.owner_actor_id || "").trim();
  const localTz = localTimeZone();
  const schedule = parseCronToPreset(cronExpr);
  const scheduleTime = formatTimeInput(schedule.hour, schedule.minute);
  const atInput = isoToLocalDatetimeInput(atRaw);

  const title = isNewRule ? t("automation.newRule") : t("ruleEditor.editRule");

  const content = (
    <div className="fixed inset-0 z-[1000]" role="dialog" aria-modal="true">
      <div className="absolute inset-0 glass-overlay" onPointerDown={onClose} />
      <div className={settingsDialogPanelClass("xl")}>
        <div className={settingsDialogHeaderClass}>
          <div className="min-w-0">
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
              {title} <span className="font-mono">{ruleId || t("ruleEditor.unnamed")}</span>
            </div>
            {!isNewRule ? (
              <div className="mt-1 text-[11px] text-[var(--color-text-tertiary)]">
                {t("ruleEditor.last")} {status.last_fired_at || "—"} • {t("ruleEditor.next")} {status.next_fire_at || "—"}{" "}
                {status.completed ? `• ${t("ruleEditor.completed")} ${status.completed_at || status.last_fired_at || "—"}` : ""}{" "}
                {status.last_error ? `• ${t("ruleEditor.error")} ${status.last_error_at || "—"}` : ""}
              </div>
            ) : null}
          </div>

          <div className="ml-auto flex items-center gap-2 flex-wrap justify-end">
            <label className="text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => patchRule({ enabled: e.target.checked })}
              />
              {t("ruleList.on")}
            </label>
            <button type="button" className={secondaryButtonClass("sm")} onClick={onClose}>
              {t("common:close")}
            </button>
          </div>
        </div>

        {errorMessage ? <div className="px-4 pt-3 text-xs text-rose-600 dark:text-rose-300">{errorMessage}</div> : null}
        {status.last_error && !isNewRule ? <div className="px-4 pt-1 text-xs text-rose-600 dark:text-rose-300">{status.last_error}</div> : null}

        <div className={settingsDialogBodyClass}>
          <div className="space-y-3">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.ruleName")}</label>
                <input
                  value={ruleId}
                  onChange={(e) => patchRule({ id: e.target.value })}
                  className={`${inputClass(isDark)} font-mono`}
                  placeholder="daily_checkin"
                  spellCheck={false}
                />
                <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                  {t("ruleEditor.ruleNameHint")}
                </div>
              </div>
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.scheduleType")}</label>
                <SelectCombobox
                  items={[
                    ...(kind === "notify" ? [{ value: "interval", label: t("ruleEditor.intervalSchedule") }] : []),
                    ...(kind === "notify" ? [{ value: "cron", label: t("ruleEditor.recurringSchedule") }] : []),
                    { value: "at", label: t("ruleEditor.oneTimeSchedule") },
                  ]}
                  value={scheduleSelectValue}
                  disabled={scheduleLockedToOneTime}
                  onChange={(value) => {
                    const nextKind = String(value || "interval");
                    if (nextKind === "cron") {
                      const nextCron = buildCronFromPreset({
                        preset: schedule.preset,
                        hour: schedule.hour,
                        minute: schedule.minute,
                        weekday: schedule.weekday,
                        dayOfMonth: schedule.dayOfMonth,
                      });
                      patchRule({
                        trigger: {
                          kind: "cron",
                          cron: cronExpr || nextCron,
                          timezone: localTz,
                        },
                      });
                      return;
                    }
                    if (nextKind === "at") {
                      onSetOneShotMode("after");
                      patchRule({ trigger: { kind: "at", at: atRaw || new Date(Date.now() + 30 * 60 * 1000).toISOString() } });
                      return;
                    }
                    patchRule({ trigger: { kind: "interval", every_seconds: everySeconds } });
                  }}
                  ariaLabel={t("ruleEditor.scheduleType")}
                  className={inputClass(isDark)}
                />
                <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
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
            <div className="text-[11px] text-amber-700 dark:text-amber-300">
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
                    patchRule({
                      trigger: {
                        kind: "interval",
                        every_seconds: Math.max(1, Number(e.target.value || 1)) * 60,
                      },
                    })
                  }
                  className={inputClass(isDark)}
                />
              </div>
              <div className="self-end text-[11px] text-[var(--color-text-muted)]">
                {t("ruleEditor.currentCadence", { duration: formatDuration(everySeconds, t) })}
              </div>
            </div>
          ) : null}

          {activeTriggerKind === "cron" ? (
            <div className="space-y-3">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.pattern")}</label>
                  <SelectCombobox
                    items={[
                      { value: "daily", label: t("ruleEditor.daily") },
                      { value: "weekly", label: t("ruleEditor.weekly") },
                      { value: "monthly", label: t("ruleEditor.monthly") },
                    ]}
                    value={schedule.preset}
                    onChange={(value) => {
                      const preset = String(value || "daily") as SchedulePreset;
                      patchRule({
                        trigger: {
                          kind: "cron",
                          cron: buildCronFromPreset({
                            preset,
                            hour: schedule.hour,
                            minute: schedule.minute,
                            weekday: schedule.weekday,
                            dayOfMonth: schedule.dayOfMonth,
                          }),
                          timezone: localTz,
                        },
                      });
                    }}
                    ariaLabel={t("ruleEditor.pattern")}
                    className={inputClass(isDark)}
                  />
                </div>
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.time")}</label>
                  <input
                    type="time"
                    value={scheduleTime}
                    onChange={(e) => {
                      const parsed = parseTimeInput(e.target.value);
                      patchRule({
                        trigger: {
                          kind: "cron",
                          cron: buildCronFromPreset({
                            preset: schedule.preset,
                            hour: parsed.hour,
                            minute: parsed.minute,
                            weekday: schedule.weekday,
                            dayOfMonth: schedule.dayOfMonth,
                          }),
                          timezone: localTz,
                        },
                      });
                    }}
                    className={inputClass(isDark)}
                  />
                </div>
              </div>

              {schedule.preset === "weekly" ? (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.weekday")}</label>
                  <SelectCombobox
                    items={weekdayOptions.map((day) => ({ value: String(day.value), label: day.label }))}
                    value={String(schedule.weekday)}
                    onChange={(value) =>
                      patchRule({
                        trigger: {
                          kind: "cron",
                          cron: buildCronFromPreset({
                            preset: "weekly",
                            hour: schedule.hour,
                            minute: schedule.minute,
                            weekday: Number(value || 1),
                            dayOfMonth: schedule.dayOfMonth,
                          }),
                          timezone: localTz,
                        },
                      })
                    }
                    ariaLabel={t("ruleEditor.weekday")}
                    className={inputClass(isDark)}
                  />
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
                    onChange={(e) =>
                      patchRule({
                        trigger: {
                          kind: "cron",
                          cron: buildCronFromPreset({
                            preset: "monthly",
                            hour: schedule.hour,
                            minute: schedule.minute,
                            weekday: schedule.weekday,
                            dayOfMonth: Number(e.target.value || 1),
                          }),
                          timezone: localTz,
                        },
                      })
                    }
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
                <SelectCombobox
                  items={[
                    { value: "after", label: t("ruleEditor.afterCountdown") },
                    { value: "exact", label: t("ruleEditor.exactTime") },
                  ]}
                  value={oneShotMode}
                  onChange={(value) => onSetOneShotMode(String(value || "after") as "after" | "exact")}
                  ariaLabel={t("ruleEditor.oneTimeMode")}
                  className={inputClass(isDark)}
                />
              </div>

              {oneShotMode === "after" ? (
                <div className="space-y-2">
                  <div className="flex flex-wrap gap-2">
                    {[5, 10, 30, 60, 120].map((minutes) => (
                      <button
                        key={minutes}
                        type="button"
                        className={secondaryButtonClass("sm")}
                        onClick={() => onSetOneShotAfterMinutes(minutes)}
                      >
                        {minutes >= 60 ? `${Math.round(minutes / 60)}h` : `${minutes}m`}
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
                      onChange={(e) => onSetOneShotAfterMinutes(clampInt(Number(e.target.value || 1), 1, 7 * 24 * 60))}
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
                    onChange={(e) => patchRule({ trigger: { kind: "at", at: localDatetimeInputToIso(e.target.value) } })}
                    className={inputClass(isDark)}
                  />
                </div>
              )}

              <div className="text-[11px] text-[var(--color-text-muted)]">
                {t("automation.savedSendTime")} <span className="font-mono break-all">{atRaw || "—"}</span>
              </div>
            </div>
          ) : null}

          <div>
            <label className={labelClass(isDark)}>{t("ruleEditor.action")}</label>
            <SelectCombobox
              items={[
                { value: "notify", label: t("ruleEditor.sendReminder") },
                {
                  value: "group_state",
                  label: `${t("ruleEditor.setGroupStatus")}${operationalActionsEnabled ? "" : t("automation.oneTimeOnlySuffix")}`,
                  disabled: !operationalActionsEnabled,
                },
                {
                  value: "actor_control",
                  label: `${t("ruleEditor.controlActorRuntimes")}${operationalActionsEnabled ? "" : t("automation.oneTimeOnlySuffix")}`,
                  disabled: !operationalActionsEnabled,
                },
              ]}
              value={kind}
              onChange={(value) => {
                const next = String(value || "notify");
                if (next !== "notify" && !operationalActionsEnabled) {
                  onSetRulesErr(t("automation.operationalActionsHint"));
                  return;
                }
                onSetRulesErr("");
                if (next === "group_state") {
                  onSetOneShotMode("after");
                  patchRule({
                    action: defaultGroupStateAction(),
                    trigger: { kind: "at", at: atRaw || buildDefaultOneShotAt() },
                  });
                  return;
                }
                if (next === "actor_control") {
                  onSetOneShotMode("after");
                  patchRule({
                    action: defaultActorControlAction(),
                    trigger: { kind: "at", at: atRaw || buildDefaultOneShotAt() },
                  });
                  return;
                }
                patchRule({
                  action: defaultNotifyAction(),
                  to: recipients.length > 0 ? recipients : ["@foreman"],
                });
              }}
              ariaLabel={t("ruleEditor.action")}
              className={inputClass(isDark)}
            />
            {!operationalActionsEnabled ? (
              <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{t("automation.operationalActionsOnly")}</div>
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
                      onRemove={() => patchRule({ to: recipients.filter((item) => item !== token) })}
                    />
                  ))}
                  <GroupCombobox
                    items={availableRecipientOptions.map((option) => ({
                      value: option.value,
                      label: option.label,
                    }))}
                    value=""
                    onChange={(value) => {
                      if (!value || recipients.includes(value)) return;
                      patchRule({ to: [...recipients, value] });
                    }}
                    placeholder={t("automation.addRecipient")}
                    searchPlaceholder={t("automation.addRecipient")}
                    emptyText={t("automation.noRecipientsAvailable", { defaultValue: "No recipients available." })}
                    ariaLabel={t("automation.addRecipient")}
                    triggerClassName="glass-input min-h-[44px] min-w-[17rem] px-3 py-2 text-sm text-[var(--color-text-primary)]"
                    contentClassName="min-w-[18rem]"
                    matchTriggerWidth
                  />
                </div>
              </div>

              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.notificationSource")}</label>
                <SelectCombobox
                  items={[
                    { value: "snippet", label: t("ruleEditor.messageSnippet") },
                    { value: "custom", label: t("ruleEditor.typeText") },
                  ]}
                  value={contentMode}
                  onChange={(value) => {
                    const nextMode = String(value || "custom");
                    if (nextMode === "snippet") {
                      if (snippetIds.length === 0) {
                        onSetRulesErr(t("automation.createSnippetFirst"));
                        return;
                      }
                      onSetRulesErr("");
                      patchRule({ action: { ...notifyAction, snippet_ref: snippetRef || snippetIds[0] || null } });
                      return;
                    }
                    onSetRulesErr("");
                    patchRule({ action: { ...notifyAction, snippet_ref: null } });
                  }}
                  ariaLabel={t("ruleEditor.notificationSource")}
                  className={inputClass(isDark)}
                />
              </div>

              {contentMode === "snippet" ? (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.selectSnippet")}</label>
                  <GroupCombobox
                    items={snippetItems}
                    value={snippetRef}
                    onChange={(value) => patchRule({ action: { ...notifyAction, snippet_ref: value || null } })}
                    placeholder={t("ruleEditor.selectSnippetPlaceholder", { defaultValue: "Select snippet" })}
                    searchPlaceholder={t("ruleEditor.searchSnippetPlaceholder", { defaultValue: "Search snippets..." })}
                    emptyText={t("automation.noSnippetsYet")}
                    ariaLabel={t("ruleEditor.selectSnippet")}
                    triggerClassName={`${inputClass(isDark)} font-mono justify-between`}
                    contentClassName="w-[min(36rem,calc(100vw-1.5rem))]"
                    descriptionClassName="font-sans"
                    matchTriggerWidth
                  />
                  {snippetIds.length === 0 ? (
                    <div className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">{t("automation.noSnippetsYet")}</div>
                  ) : null}
                  {snippetRef ? (
                    <div className="mt-3">
                      <div className="mb-2 flex items-center justify-between gap-2">
                        <div className="text-[11px] font-medium text-[var(--color-text-secondary)]">
                          {t("ruleEditor.snippetPreview", { defaultValue: "Snippet Preview" })}
                        </div>
                        <div className="rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2 py-0.5 text-[10px] font-mono text-[var(--color-text-secondary)]">
                          {snippetRef}
                        </div>
                      </div>
                      <div className={`${inputClass(isDark)} min-h-[120px] whitespace-pre-wrap font-mono text-[12px] leading-6`}>
                        {selectedSnippetContent || t("ruleEditor.snippetPreviewEmpty", { defaultValue: "This snippet is empty." })}
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : (
                <div>
                  <label className={labelClass(isDark)}>{t("ruleEditor.messageLabel")}</label>
                  <textarea
                    value={message}
                    onChange={(e) => patchRule({ action: { ...notifyAction, message: e.target.value } })}
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
              <SelectCombobox
                items={[
                  { value: "active", label: groupStateCopy.active.label },
                  { value: "idle", label: groupStateCopy.idle.label },
                  { value: "paused", label: groupStateCopy.paused.label },
                  { value: "stopped", label: groupStateCopy.stopped.label },
                ]}
                value={groupStateValue}
                onChange={(value) =>
                  patchRule({
                    action: {
                      kind: "group_state",
                      state: String(value || "paused") as "active" | "idle" | "paused" | "stopped",
                    },
                  })
                }
                ariaLabel={t("ruleEditor.groupStatusTarget")}
                className={inputClass(isDark)}
              />
              <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                {groupStateCopy[(groupStateValue as "active" | "idle" | "paused" | "stopped") || "paused"].hint}
              </div>
            </div>
          ) : null}

          {kind === "actor_control" ? (
            <div className="space-y-3">
              <div>
                <label className={labelClass(isDark)}>{t("ruleEditor.runtimeOperation")}</label>
                <SelectCombobox
                  items={[
                    { value: "start", label: actorOperationCopy.start.label },
                    { value: "stop", label: actorOperationCopy.stop.label },
                    { value: "restart", label: actorOperationCopy.restart.label },
                  ]}
                  value={actorOperation}
                  onChange={(value) =>
                    patchRule({
                      action: {
                        kind: "actor_control",
                        operation: String(value || "restart") as "start" | "stop" | "restart",
                        targets: actorTargets.length > 0 ? actorTargets : ["@all"],
                      },
                    })
                  }
                  ariaLabel={t("ruleEditor.runtimeOperation")}
                  className={inputClass(isDark)}
                />
                <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">
                  {actorOperationCopy[(actorOperation as "start" | "stop" | "restart") || "restart"].hint}
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
                        patchRule({
                          action: {
                            kind: "actor_control",
                            operation: actorOperation as "start" | "stop" | "restart",
                            targets: actorTargets.filter((item) => item !== token),
                          },
                        })
                      }
                    />
                  ))}
                  <GroupCombobox
                    items={availableActorTargetOptions.map((option) => ({
                      value: option.value,
                      label: option.label,
                    }))}
                    value=""
                    onChange={(value) => {
                      if (!value || actorTargets.includes(value)) return;
                      patchRule({
                        action: {
                          kind: "actor_control",
                          operation: actorOperation as "start" | "stop" | "restart",
                          targets: [...actorTargets, value],
                        },
                      });
                    }}
                    placeholder={t("automation.addTarget")}
                    searchPlaceholder={t("automation.addTarget")}
                    emptyText={t("automation.noTargetsAvailable", { defaultValue: "No targets available." })}
                    ariaLabel={t("automation.addTarget")}
                    triggerClassName="glass-input min-h-[44px] min-w-[17rem] px-3 py-2 text-sm text-[var(--color-text-primary)]"
                    contentClassName="min-w-[18rem]"
                    matchTriggerWidth
                  />
                </div>
              </div>
            </div>
            ) : null}
          </div>
        </div>
        <div className={settingsDialogFooterClass}>
          <button type="button" className={secondaryButtonClass()} onClick={onClose} disabled={saveBusy}>
            {t("common:cancel")}
          </button>
          <button type="button" className={primaryButtonClass(saveBusy)} onClick={() => void onSave()} disabled={saveBusy}>
            {saveBusy ? t("common:saving") : t("common:save")}
          </button>
        </div>
      </div>
    </div>
  );

  return <BodyPortal>{content}</BodyPortal>;
}
