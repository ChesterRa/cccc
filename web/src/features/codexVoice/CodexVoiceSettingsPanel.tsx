import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { CodexVoiceAnalystSettingsFields } from "./CodexVoiceAnalystSettings";
import { CodexVoiceAudioSettings } from "./CodexVoiceAudioSettings";
import { CodexVoicePreferenceFields } from "./CodexVoicePreferenceFields";
import { useVoicePreferences } from "./useVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

type SettingsSection = "audio" | "notifications" | "analyst";
type Props = {
  active: boolean;
  controller: CodexVoiceSessionController;
  analystSettings: import("./useCodexVoiceShell").CodexVoiceShellState["analystSettings"];
  onAnalystSettingsActive: (active: boolean) => void;
  onProfileEditingChange?: (editing: boolean) => void;
};
const SETTINGS_SECTIONS: SettingsSection[] = ["audio", "notifications", "analyst"];

// The shared editor uses the existing call controller. Analyst drafts live
// above the settings surface and survive closing or changing settings tabs.
export function CodexVoiceSettingsPanel({
  active,
  controller,
  analystSettings,
  onAnalystSettingsActive,
  onProfileEditingChange,
}: Props) {
  const { t } = useTranslation("modals");
  const [section, setSection] = useState<SettingsSection>("audio");
  const preferences = useVoicePreferences(active);
  const tabRefs = useRef<Partial<Record<SettingsSection, HTMLButtonElement>>>({});
  useEffect(() => {
    onAnalystSettingsActive(active && section === "analyst");
    return () => onAnalystSettingsActive(false);
  }, [active, section, onAnalystSettingsActive]);
  const selectAdjacentSection = (
    event: KeyboardEvent<HTMLButtonElement>,
    current: SettingsSection,
  ) => {
    const index = SETTINGS_SECTIONS.indexOf(current);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? 2
          : event.key === "ArrowRight"
            ? (index + 1) % 3
            : event.key === "ArrowLeft"
              ? (index + 2) % 3
              : undefined;
    if (next === undefined) return;
    event.preventDefault();
    setSection(SETTINGS_SECTIONS[next]);
    tabRefs.current[SETTINGS_SECTIONS[next]]?.focus();
  };
  return (
    <section className="@container/voice-settings min-w-0" data-codex-voice-settings-panel="true">
      <div
        className="flex flex-none border-b border-[var(--glass-border-subtle)]"
        role="tablist"
        aria-label={t("codexVoiceSettings")}
      >
        {SETTINGS_SECTIONS.map((candidate) => (
          <button
            key={candidate}
            ref={(node) => {
              if (node) tabRefs.current[candidate] = node;
            }}
            type="button"
            role="tab"
            id={`codex-voice-settings-${candidate}-tab`}
            aria-controls={`codex-voice-settings-${candidate}-panel`}
            aria-selected={section === candidate}
            tabIndex={section === candidate ? 0 : -1}
            className={`min-w-0 flex-1 border-b-2 px-1 py-3 text-xs font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] sm:flex-none sm:px-4 ${section === candidate ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]" : "border-transparent text-[var(--color-text-muted)]"}`}
            onClick={() => setSection(candidate)}
            onKeyDown={(event) => selectAdjacentSection(event, candidate)}
          >
            {t(
              candidate === "audio"
                ? "codexVoiceSettingsVoiceAudio"
                : candidate === "notifications"
                  ? "voicePreferences.notifications"
                  : "codexVoiceAnalystTitle",
            )}
          </button>
        ))}
      </div>
      <div className="min-w-0 pt-5">
        <div
          id="codex-voice-settings-audio-panel"
          role="tabpanel"
          className="min-w-0 space-y-5"
          aria-labelledby="codex-voice-settings-audio-tab"
          hidden={section !== "audio"}
        >
          <CodexVoiceAudioSettings controller={controller} />
          <CodexVoicePreferenceFields preferences={preferences} section="audio" />
        </div>
        <div
          id="codex-voice-settings-notifications-panel"
          role="tabpanel"
          className="min-w-0"
          aria-labelledby="codex-voice-settings-notifications-tab"
          hidden={section !== "notifications"}
        >
          <CodexVoicePreferenceFields preferences={preferences} section="notifications" />
        </div>
        <div
          id="codex-voice-settings-analyst-panel"
          role="tabpanel"
          className="min-w-0"
          aria-labelledby="codex-voice-settings-analyst-tab"
          hidden={section !== "analyst"}
        >
          <CodexVoiceAnalystSettingsFields
            form={analystSettings}
            controller={controller}
            onProfileEditingChange={onProfileEditingChange}
          />
        </div>
      </div>
    </section>
  );
}
