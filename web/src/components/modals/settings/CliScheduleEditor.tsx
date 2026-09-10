import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { CliSchedule } from "../../../services/api";
import { SelectCombobox } from "../../SelectCombobox";
import {
  buildCronFromPreset,
  formatTimeInput,
  getWeekdayOptions,
  isoToLocalDatetimeInput,
  isValidId,
  localDatetimeInputToIso,
  localTimeZone,
  parseCronToPreset,
  parseTimeInput,
} from "./automationUtils";
import type { SchedulePreset } from "./automationUtils";
import { inputClass, labelClass, primaryButtonClass, secondaryButtonClass } from "./types";

function cronPreset(cron: string) {
  const schedule = parseCronToPreset(cron);
  const normalized = cron
    .trim()
    .split(/\s+/)
    .map((part, index) =>
      index === 4 && Number(part) === 7 ? "0" : /^\d+$/.test(part) ? String(Number(part)) : part,
    )
    .join(" ");
  return { schedule, compatible: buildCronFromPreset(schedule) === normalized };
}

export function CliCronSummary({ cron, timezone }: { cron: string; timezone: string }) {
  const { t } = useTranslation("settings");
  const { schedule, compatible } = cronPreset(cron);
  const time = formatTimeInput(schedule.hour, schedule.minute);
  const text = !compatible
    ? cron
    : t(`cliManagement.schedule${schedule.preset}`, {
        time,
        day: schedule.dayOfMonth,
        weekday: getWeekdayOptions(t).find((day) => day.value === schedule.weekday)?.label,
      });
  return (
    <span>
      {text} ({timezone})
    </span>
  );
}

// 复用现有调度表单词汇和时间转换，不构造 Actor 提醒或工作组自动化动作。
export function CliScheduleEditor({
  initial,
  busy,
  onSave,
  onCancel,
}: {
  initial: CliSchedule;
  busy: boolean;
  onSave: (rule: CliSchedule) => Promise<void>;
  onCancel: () => void;
}) {
  const { t } = useTranslation("settings");
  const [draft, setDraft] = useState(initial);
  const [oneShotMode, setOneShotMode] = useState<"after" | "exact">("exact");
  const [minutes, setMinutes] = useState(30);
  const [error, setError] = useState("");
  const trigger = draft.trigger;
  const { schedule, compatible: presetCompatible } = cronPreset(
    trigger.kind === "cron" ? trigger.cron : "0 3 * * *",
  );
  const timezone = trigger.kind === "cron" ? trigger.timezone || "UTC" : localTimeZone();
  const updateCron = (patch: Partial<typeof schedule>) => {
    setDraft({
      ...draft,
      trigger: { kind: "cron", cron: buildCronFromPreset({ ...schedule, ...patch }), timezone },
    });
  };

  return (
    <form
      className="space-y-3"
      onSubmit={(event) => {
        event.preventDefault();
        setError("");
        if (!isValidId(draft.id)) {
          setError(t("cliManagement.invalidScheduleName"));
          return;
        }
        const next =
          trigger.kind === "at" && oneShotMode === "after"
            ? {
                ...draft,
                trigger: {
                  kind: "at" as const,
                  at: new Date(Date.now() + minutes * 60_000).toISOString(),
                },
              }
            : draft;
        if (
          next.enabled &&
          next.trigger.kind === "at" &&
          !(Date.parse(next.trigger.at) > Date.now())
        ) {
          setError(t("cliManagement.futureTimeRequired"));
          return;
        }
        void onSave(next);
      }}
    >
      <fieldset disabled={busy} className="space-y-3">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <label className={labelClass()}>
            {t("ruleEditor.ruleName")}
            <input
              required
              maxLength={64}
              value={draft.id}
              onChange={(e) => setDraft({ ...draft, id: e.target.value })}
              className={inputClass()}
            />
          </label>
          <div>
            <label className={labelClass()}>{t("ruleEditor.scheduleType")}</label>
            <SelectCombobox
              items={[
                { value: "interval", label: t("ruleEditor.intervalSchedule") },
                { value: "cron", label: t("ruleEditor.recurringSchedule") },
                { value: "at", label: t("ruleEditor.oneTimeSchedule") },
              ]}
              value={trigger.kind}
              disabled={busy}
              ariaLabel={t("ruleEditor.scheduleType")}
              className={inputClass()}
              onChange={(kind) => {
                setOneShotMode("after");
                setDraft({
                  ...draft,
                  trigger:
                    kind === "at"
                      ? { kind: "at", at: new Date(Date.now() + 30 * 60_000).toISOString() }
                      : kind === "interval"
                        ? { kind: "interval", every_seconds: 3600 }
                        : { kind: "cron", cron: "0 3 * * *", timezone: localTimeZone() },
                });
              }}
            />
          </div>
        </div>
        {trigger.kind === "interval" && (
          <label className={labelClass()}>
            {t("ruleEditor.repeatEvery")}
            <input
              required
              type="number"
              min={1}
              max={525600}
              value={trigger.every_seconds / 60}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  trigger: { kind: "interval", every_seconds: Number(e.target.value) * 60 },
                })
              }
              className={inputClass()}
            />
          </label>
        )}
        {trigger.kind === "cron" && (
          <>
            {!presetCompatible && (
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("cliManagement.customScheduleHint")}
              </p>
            )}
            {presetCompatible && (
              <>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    <label className={labelClass()}>{t("ruleEditor.pattern")}</label>
                    <SelectCombobox
                      items={["daily", "weekly", "monthly"].map((value) => ({
                        value,
                        label: t(`ruleEditor.${value}`),
                      }))}
                      value={schedule.preset}
                      disabled={busy}
                      ariaLabel={t("ruleEditor.pattern")}
                      className={inputClass()}
                      onChange={(value) => updateCron({ preset: value as SchedulePreset })}
                    />
                  </div>
                  <label className={labelClass()}>
                    {t("ruleEditor.time")}
                    <input
                      required
                      type="time"
                      value={formatTimeInput(schedule.hour, schedule.minute)}
                      onChange={(e) => updateCron(parseTimeInput(e.target.value))}
                      className={inputClass()}
                    />
                  </label>
                </div>
                {schedule.preset === "weekly" && (
                  <div>
                    <label className={labelClass()}>{t("ruleEditor.weekday")}</label>
                    <SelectCombobox
                      items={getWeekdayOptions(t).map((day) => ({
                        value: String(day.value),
                        label: day.label,
                      }))}
                      value={String(schedule.weekday)}
                      disabled={busy}
                      ariaLabel={t("ruleEditor.weekday")}
                      className={inputClass()}
                      onChange={(value) => updateCron({ weekday: Number(value) })}
                    />
                  </div>
                )}
                {schedule.preset === "monthly" && (
                  <label className={labelClass()}>
                    {t("ruleEditor.dayOfMonth")}
                    <input
                      required
                      type="number"
                      min={1}
                      max={31}
                      value={schedule.dayOfMonth}
                      onChange={(e) => updateCron({ dayOfMonth: Number(e.target.value) })}
                      className={inputClass()}
                    />
                  </label>
                )}
              </>
            )}
            <label className={labelClass()}>
              {t("cliManagement.timezone")}
              <input
                required
                value={timezone}
                onChange={(e) =>
                  setDraft({ ...draft, trigger: { ...trigger, timezone: e.target.value } })
                }
                className={inputClass()}
                spellCheck={false}
              />
            </label>
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t("cliManagement.scheduleTimeHint")}
            </p>
            <p className="text-xs break-all">
              <CliCronSummary cron={trigger.cron} timezone={timezone} />
            </p>
          </>
        )}
        {trigger.kind === "at" && (
          <>
            <div>
              <label className={labelClass()}>{t("ruleEditor.oneTimeMode")}</label>
              <SelectCombobox
                items={[
                  { value: "after", label: t("ruleEditor.afterCountdown") },
                  { value: "exact", label: t("ruleEditor.exactTime") },
                ]}
                value={oneShotMode}
                disabled={busy}
                ariaLabel={t("ruleEditor.oneTimeMode")}
                className={inputClass()}
                onChange={(value) => setOneShotMode(value === "after" ? "after" : "exact")}
              />
            </div>
            {oneShotMode === "after" ? (
              <>
                <div className="flex flex-wrap gap-2">
                  {[5, 10, 30, 60, 120].map((value) => (
                    <button
                      type="button"
                      key={value}
                      className={secondaryButtonClass("sm")}
                      onClick={() => setMinutes(value)}
                    >
                      {value >= 60 ? `${value / 60}h` : `${value}m`}
                    </button>
                  ))}
                </div>
                <label className={labelClass()}>
                  {t("cliManagement.runAfterMinutes")}
                  <input
                    required
                    type="number"
                    min={1}
                    max={10080}
                    value={minutes}
                    onChange={(e) => setMinutes(Number(e.target.value))}
                    className={inputClass()}
                  />
                </label>
              </>
            ) : (
              <label className={labelClass()}>
                {t("ruleEditor.dateTime")}
                <input
                  required
                  type="datetime-local"
                  value={isoToLocalDatetimeInput(trigger.at)}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      trigger: { kind: "at", at: localDatetimeInputToIso(e.target.value) },
                    })
                  }
                  className={inputClass()}
                />
              </label>
            )}
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t("cliManagement.localTime", { timezone: localTimeZone() })}
            </p>
          </>
        )}
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          />
          {t("ruleList.on")}
        </label>
      </fieldset>
      {error && (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {error}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <button type="submit" disabled={busy} className={primaryButtonClass()}>
          {busy ? t("common:saving") : t("common:save")}
        </button>
        <button type="button" disabled={busy} className={secondaryButtonClass()} onClick={onCancel}>
          {t("common:cancel")}
        </button>
      </div>
    </form>
  );
}
