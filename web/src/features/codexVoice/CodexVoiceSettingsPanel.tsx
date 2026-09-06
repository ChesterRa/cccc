import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeftIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { CodexVoiceAnalystSettings } from "./CodexVoiceAnalystSettings";
import { CodexVoiceAudioSettings } from "./CodexVoiceAudioSettings";
import { CodexVoicePreferenceFields } from "./CodexVoicePreferenceFields";
import { useVoicePreferences } from "./useVoicePreferences";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

type SettingsSection = "audio" | "notifications" | "analyst";
type Props = { active: boolean; controller: CodexVoiceSessionController; onClose: () => void };
const SETTINGS_SECTIONS: SettingsSection[] = ["audio", "notifications", "analyst"];

// Mounted for the lifetime of the Voice panel once first opened. Navigation
// changes visibility, so returning to the conversation never discards a draft.
export function CodexVoiceSettingsPanel({ active, controller, onClose }: Props) {
  const { t } = useTranslation("modals");
  const [section, setSection] = useState<SettingsSection>("audio");
  const preferences = useVoicePreferences(active);
  const tabRefs = useRef<Partial<Record<SettingsSection, HTMLButtonElement>>>({});
  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => tabRefs.current[section]?.focus());
    return () => cancelAnimationFrame(frame);
  }, [active, section]);

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
          : event.key === "ArrowRight" || event.key === "ArrowDown"
            ? (index + 1) % 3
            : event.key === "ArrowLeft" || event.key === "ArrowUp"
              ? (index + 2) % 3
              : undefined;
    if (next === undefined) return;
    event.preventDefault();
    setSection(SETTINGS_SECTIONS[next]);
  };

  return (
    <section
      className="flex h-full min-h-0 flex-col bg-[var(--color-bg-primary)]"
      aria-labelledby="codex-voice-settings-title"
      data-codex-voice-settings-panel="true"
    >
      <div className="flex flex-none flex-wrap items-center gap-x-4 gap-y-1 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5">
        <Button type="button" variant="ghost" size="sm" onClick={onClose}>
          <ChevronLeftIcon size={16} />
          {t("codexVoiceBackToConversation")}
        </Button>
        <h3 id="codex-voice-settings-title" className="text-sm font-semibold">
          {t("codexVoiceSettings")}
        </h3>
        <p className="w-full px-2 text-xs leading-5 text-[var(--color-text-muted)] sm:w-auto sm:px-0">
          {t("codexVoiceSettingsHint")}
        </p>
      </div>
      <div
        className="flex flex-none border-b border-[var(--glass-border-subtle)] px-3 sm:px-5"
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
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div
          id="codex-voice-settings-audio-panel"
          role="tabpanel"
          aria-labelledby="codex-voice-settings-audio-tab"
          hidden={section !== "audio"}
        >
          <CodexVoiceAudioSettings active={active && section === "audio"} controller={controller} />
          <CodexVoicePreferenceFields preferences={preferences} section="audio" />
        </div>
        <div
          id="codex-voice-settings-notifications-panel"
          role="tabpanel"
          aria-labelledby="codex-voice-settings-notifications-tab"
          hidden={section !== "notifications"}
        >
          <CodexVoicePreferenceFields preferences={preferences} section="notifications" />
        </div>
        <div
          id="codex-voice-settings-analyst-panel"
          role="tabpanel"
          aria-labelledby="codex-voice-settings-analyst-tab"
          hidden={section !== "analyst"}
        >
          <CodexVoiceAnalystSettings
            active={active && section === "analyst"}
            controller={controller}
          />
        </div>
      </div>
    </section>
  );
}
