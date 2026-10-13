import { useState, useId } from "react";
import { useTranslation } from "react-i18next";
import { SegmentedControl } from "../../components/ui/segmented-control";
import {
  SettingsCard,
  SettingsRow,
  SettingsSwitch,
} from "../../components/modals/settings/SettingsKit";
import { useGroupStore } from "../../stores/useGroupStore";
import type { VoicePreferences, VoiceNotificationScope } from "../../services/api/codexVoice";
import type { useVoicePreferences } from "./useVoicePreferences";

type Props = {
  preferences: ReturnType<typeof useVoicePreferences>;
  section: "audio" | "notifications";
};

const VERBOSITY = ["concise", "standard", "detailed"] as const;
const STYLE = ["natural", "direct", "patient"] as const;
const SCOPES = ["off", "to_user", "all_chat"] as const;

export function CodexVoicePreferenceFields({ preferences, section }: Props) {
  const { t } = useTranslation("modals");
  const suppressLabel = useId();
  const groups = useGroupStore((state) => state.groups);
  const [search, setSearch] = useState("");
  const { value, saving, saved, error, change } = preferences;
  const options = <T extends string>(values: readonly T[]) =>
    values.map((option) => ({ value: option, label: t(`voicePreferences.${option}`) }));
  const visibleGroups = groups.filter((group) =>
    `${group.title ?? ""} ${group.group_id}`.toLowerCase().includes(search.trim().toLowerCase()),
  );
  const status =
    saving || saved || error ? (
      <p
        role={error ? "alert" : "status"}
        className={`text-xs ${error ? "text-rose-600 dark:text-rose-400" : "text-[var(--color-text-muted)]"}`}
      >
        {error || t(saving ? "voicePreferences.saving" : "voicePreferences.saved")}
      </p>
    ) : null;

  if (!value) {
    // A failed first load has no form to attach the error to, so show it in place of loading.
    return error ? (
      status
    ) : (
      <p role="status" className="text-sm text-[var(--color-text-secondary)]">
        {t("voicePreferences.loading")}
      </p>
    );
  }

  if (section === "audio") {
    return (
      <SettingsCard
        title={t("voicePreferences.responseSection")}
        description={t("voicePreferences.nextCall")}
        actions={status}
      >
        <div className="divide-y divide-[var(--glass-border-subtle)]">
          <SettingsRow label={t("voicePreferences.verbosity")}>
            <SegmentedControl
              label={t("voicePreferences.verbosity")}
              value={value.verbosity}
              options={options(VERBOSITY)}
              disabled={saving}
              onChange={(verbosity: VoicePreferences["verbosity"]) => void change({ verbosity })}
            />
          </SettingsRow>
          <SettingsRow label={t("voicePreferences.style")}>
            <SegmentedControl
              label={t("voicePreferences.style")}
              value={value.style}
              options={options(STYLE)}
              disabled={saving}
              onChange={(style: VoicePreferences["style"]) => void change({ style })}
            />
          </SettingsRow>
        </div>
      </SettingsCard>
    );
  }

  const enabledCount = Object.values(value.groups).filter((scope) => scope !== "off").length;
  return (
    <div className="space-y-5">
      <SettingsCard
        title={t("voicePreferences.announceSection")}
        description={t("voicePreferences.notificationHint")}
        actions={status}
      >
        <SettingsRow label={t("voicePreferences.suppressViewed")} labelId={suppressLabel}>
          <SettingsSwitch
            labelledBy={suppressLabel}
            checked={value.suppress_viewed}
            disabled={saving}
            onChange={(checked) => void change({ suppress_viewed: checked })}
          />
        </SettingsRow>
        <details className="px-5 py-3.5 text-xs leading-5 text-[var(--color-text-secondary)]">
          <summary className="w-fit cursor-pointer py-1 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]">
            {t("voicePreferences.viewedHelp")}
          </summary>
          <p className="mt-1">{t("voicePreferences.viewedHint")}</p>
        </details>
      </SettingsCard>

      <SettingsCard
        title={t("voicePreferences.groupsSection")}
        actions={
          <span
            className={`rounded-full px-2.5 py-1 text-xs font-medium ${enabledCount ? "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300" : "bg-[var(--color-bg-secondary)] text-[var(--color-text-muted)]"}`}
          >
            {t("voicePreferences.enabledGroups", { count: enabledCount })}
          </span>
        }
      >
        <div className="border-b border-[var(--glass-border-subtle)] p-3">
          <input
            type="search"
            aria-label={t("voicePreferences.searchGroups")}
            placeholder={t("voicePreferences.searchGroups")}
            className="min-h-9 w-full rounded-lg px-3 text-sm glass-input text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        {visibleGroups.length ? (
          <ul className="@container/voice-groups divide-y divide-[var(--glass-border-subtle)]">
            {visibleGroups.map((group) => {
              const name = group.title || group.group_id;
              const scope = value.groups[group.group_id] ?? "off";
              return (
                <li
                  key={group.group_id}
                  className="flex flex-col gap-2 px-4 py-2.5 @min-[420px]/voice-groups:flex-row @min-[420px]/voice-groups:items-center @min-[420px]/voice-groups:justify-between @min-[420px]/voice-groups:gap-4"
                >
                  <span className="flex min-w-0 items-center gap-2 text-sm text-[var(--color-text-primary)]">
                    <span
                      aria-hidden="true"
                      className={`h-1.5 w-1.5 flex-none rounded-full ${scope === "off" ? "bg-[var(--color-text-tertiary)]" : "bg-emerald-400"}`}
                    />
                    <span className="truncate">{name}</span>
                  </span>
                  <SegmentedControl
                    label={name}
                    value={scope}
                    options={options(SCOPES)}
                    disabled={saving}
                    onChange={(next: VoiceNotificationScope) =>
                      void change({ groups: { ...value.groups, [group.group_id]: next } })
                    }
                  />
                </li>
              );
            })}
          </ul>
        ) : (
          <p
            role="status"
            className="px-4 py-6 text-center text-sm text-[var(--color-text-secondary)]"
          >
            {t(groups.length ? "voicePreferences.noMatchingGroups" : "voicePreferences.noGroups")}
          </p>
        )}
      </SettingsCard>
    </div>
  );
}
