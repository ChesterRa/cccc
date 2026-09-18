import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useGroupStore } from "../../stores/useGroupStore";
import type { VoicePreferences, VoiceNotificationScope } from "../../services/api/codexVoice";
import type { useVoicePreferences } from "./useVoicePreferences";

type Props = {
  preferences: ReturnType<typeof useVoicePreferences>;
  section: "audio" | "notifications";
};
const fieldClass =
  "mt-2 min-h-11 w-full rounded-lg px-3 py-2 text-sm glass-input text-[var(--color-text-primary)] disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]";

export function CodexVoicePreferenceFields({ preferences, section }: Props) {
  const { t } = useTranslation("modals");
  const groups = useGroupStore((state) => state.groups);
  const [search, setSearch] = useState("");
  const { value, saving, saved, error, change } = preferences;
  const visibleGroups = groups.filter((group) =>
    `${group.title ?? ""} ${group.group_id}`.toLowerCase().includes(search.trim().toLowerCase()),
  );
  return (
    <div className="space-y-4 px-5 py-5 text-sm text-[var(--color-text-primary)] sm:px-6">
      {saving || saved ? (
        <p role="status" className="text-xs text-[var(--color-text-muted)]">
          {t(saving ? "voicePreferences.saving" : "voicePreferences.saved")}
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="text-rose-600 dark:text-rose-400">
          {error}
        </p>
      ) : null}
      {!value ? (
        <p role="status">{t("voicePreferences.loading")}</p>
      ) : section === "audio" ? (
        <>
          <div className="grid gap-4 sm:grid-cols-2">
            <label>
              {t("voicePreferences.verbosity")}
              <select
                className={fieldClass}
                value={value.verbosity}
                disabled={saving}
                onChange={(e) =>
                  void change({ verbosity: e.target.value as VoicePreferences["verbosity"] })
                }
              >
                {(["concise", "standard", "detailed"] as const).map((option) => (
                  <option key={option} value={option}>
                    {t(`voicePreferences.${option}`)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {t("voicePreferences.style")}
              <select
                className={fieldClass}
                value={value.style}
                disabled={saving}
                onChange={(e) =>
                  void change({ style: e.target.value as VoicePreferences["style"] })
                }
              >
                {(["natural", "direct", "patient"] as const).map((option) => (
                  <option key={option} value={option}>
                    {t(`voicePreferences.${option}`)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voicePreferences.nextCall")}
          </p>
        </>
      ) : (
        <>
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voicePreferences.notificationHint")}
          </p>
          <p className="text-xs font-medium">
            {t("voicePreferences.enabledGroups", {
              count: Object.values(value.groups).filter((scope) => scope !== "off").length,
            })}
          </p>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              className="mt-1"
              checked={value.suppress_viewed}
              disabled={saving}
              onChange={(e) => void change({ suppress_viewed: e.target.checked })}
            />
            {t("voicePreferences.suppressViewed")}
          </label>
          <details className="text-xs leading-5 text-[var(--color-text-secondary)]">
            <summary className="w-fit cursor-pointer py-1 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]">
              {t("voicePreferences.viewedHelp")}
            </summary>
            <p className="mt-1">{t("voicePreferences.viewedHint")}</p>
          </details>
          <input
            type="search"
            aria-label={t("voicePreferences.searchGroups")}
            placeholder={t("voicePreferences.searchGroups")}
            className={fieldClass}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <div className="divide-y divide-[var(--glass-border-subtle)]">
            {visibleGroups.map((group) => (
              <label
                key={group.group_id}
                className="grid min-w-0 grid-cols-1 items-center gap-2 py-3 sm:grid-cols-[minmax(0,1fr)_14rem] sm:gap-4"
              >
                <span className="min-w-0 flex-1 break-words">{group.title || group.group_id}</span>
                <select
                  className={`${fieldClass} !mt-0 min-w-0`}
                  aria-label={group.title || group.group_id}
                  disabled={saving}
                  value={value.groups[group.group_id] ?? "off"}
                  onChange={(e) =>
                    void change({
                      groups: {
                        ...value.groups,
                        [group.group_id]: e.target.value as VoiceNotificationScope,
                      },
                    })
                  }
                >
                  {(["off", "to_user", "all_chat"] as const).map((option) => (
                    <option key={option} value={option}>
                      {t(`voicePreferences.${option}`)}
                    </option>
                  ))}
                </select>
              </label>
            ))}
          </div>
          {visibleGroups.length === 0 ? (
            <p role="status" className="text-sm text-[var(--color-text-secondary)]">
              {t(groups.length ? "voicePreferences.noMatchingGroups" : "voicePreferences.noGroups")}
            </p>
          ) : null}
        </>
      )}
    </div>
  );
}
