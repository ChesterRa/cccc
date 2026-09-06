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
  "mt-2 h-10 w-full rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-3 text-sm text-[var(--color-text-primary)] disabled:opacity-60";

export function CodexVoicePreferenceFields({ preferences, section }: Props) {
  const { t } = useTranslation("modals");
  const groups = useGroupStore((state) => state.groups);
  const [search, setSearch] = useState("");
  const { value, saving, saved, error, change } = preferences;
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
          <p className="text-xs leading-5 text-[var(--color-text-muted)]">
            {t("voicePreferences.viewedHint")}
          </p>
          <input
            type="search"
            aria-label={t("voicePreferences.searchGroups")}
            placeholder={t("voicePreferences.searchGroups")}
            className={fieldClass}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <div className="divide-y divide-[var(--glass-border-subtle)]">
            {groups
              .filter((group) =>
                `${group.title ?? ""} ${group.group_id}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )
              .map((group) => (
                <label
                  key={group.group_id}
                  className="flex flex-wrap items-center justify-between gap-3 py-3"
                >
                  <span className="min-w-0 flex-1 break-words">
                    {group.title || group.group_id}
                  </span>
                  <select
                    className={`${fieldClass} !mt-0 !w-44 shrink-0`}
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
        </>
      )}
    </div>
  );
}
