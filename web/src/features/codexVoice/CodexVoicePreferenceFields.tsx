import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { SegmentedControl } from "../../components/ui/segmented-control";
import { Switch } from "../../components/ui/switch";
import { useGroupStore } from "../../stores/useGroupStore";
import type { VoicePreferences, VoiceNotificationScope } from "../../services/api/codexVoice";
import { CodexVoiceSettingsSection } from "./CodexVoiceSettingsSection";
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
      <CodexVoiceSettingsSection
        title={t("voicePreferences.responseSection")}
        description={t("voicePreferences.nextCall")}
        aside={status}
      >
        <div className="grid gap-4 @min-[640px]/voice-settings:grid-cols-2 @min-[640px]/voice-settings:gap-6">
          <PreferenceRow label={t("voicePreferences.verbosity")}>
            <SegmentedControl
              label={t("voicePreferences.verbosity")}
              value={value.verbosity}
              options={options(VERBOSITY)}
              disabled={saving}
              onChange={(verbosity: VoicePreferences["verbosity"]) => void change({ verbosity })}
            />
          </PreferenceRow>
          <PreferenceRow label={t("voicePreferences.style")}>
            <SegmentedControl
              label={t("voicePreferences.style")}
              value={value.style}
              options={options(STYLE)}
              disabled={saving}
              onChange={(style: VoicePreferences["style"]) => void change({ style })}
            />
          </PreferenceRow>
        </div>
      </CodexVoiceSettingsSection>
    );
  }

  const enabledCount = Object.values(value.groups).filter((scope) => scope !== "off").length;
  return (
    <div className="grid items-start gap-4 @min-[900px]/voice-settings:gap-5 @min-[1100px]/voice-settings:grid-cols-[340px_minmax(0,1fr)]">
      <CodexVoiceSettingsSection
        title={t("voicePreferences.announceSection")}
        description={t("voicePreferences.notificationHint")}
        aside={status}
      >
        <label className="flex items-center justify-between gap-4 text-sm text-[var(--color-text-primary)]">
          <span>{t("voicePreferences.suppressViewed")}</span>
          <Switch
            checked={value.suppress_viewed}
            disabled={saving}
            onChange={(e) => void change({ suppress_viewed: e.target.checked })}
          />
        </label>
        <details className="mt-1 text-xs leading-5 text-[var(--color-text-secondary)]">
          <summary className="w-fit cursor-pointer py-1 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]">
            {t("voicePreferences.viewedHelp")}
          </summary>
          <p className="mt-1">{t("voicePreferences.viewedHint")}</p>
        </details>
      </CodexVoiceSettingsSection>

      <CodexVoiceSettingsSection
        title={t("voicePreferences.groupsSection")}
        aside={
          <span
            className={`rounded-full px-2.5 py-1 text-xs font-medium ${enabledCount ? "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300" : "bg-[var(--color-bg-secondary)] text-[var(--color-text-muted)]"}`}
          >
            {t("voicePreferences.enabledGroups", { count: enabledCount })}
          </span>
        }
        flush
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
      </CodexVoiceSettingsSection>
    </div>
  );
}

function PreferenceRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-2 [&>[role=radiogroup]]:flex [&>[role=radiogroup]]:w-full">
      <span className="text-xs font-medium text-[var(--color-text-secondary)]">{label}</span>
      {children}
    </div>
  );
}
