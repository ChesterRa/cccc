import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { BellIcon, ChevronLeftIcon, HeadphonesIcon, TerminalIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { CodexVoiceAnalystSettings } from "./CodexVoiceAnalystSettings";
import { CodexVoiceAudioSettings } from "./CodexVoiceAudioSettings";
import { CodexVoicePreferenceFields } from "./CodexVoicePreferenceFields";
import { CodexVoiceSettingsHeading } from "./CodexVoiceSettingsSection";
import { SETTINGS_COLUMN } from "./codexVoiceSettingsLayout";
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

  const sectionLabel = (candidate: SettingsSection) =>
    t(
      candidate === "audio"
        ? "codexVoiceSettingsVoiceAudio"
        : candidate === "notifications"
          ? "voicePreferences.notifications"
          : "codexVoiceAnalystTitle",
    );
  const SectionIcon = { audio: HeadphonesIcon, notifications: BellIcon, analyst: TerminalIcon };

  return (
    <section
      className="@container/voice-settings flex h-full min-h-0 flex-col bg-[var(--color-bg-primary)]"
      aria-labelledby="codex-voice-settings-title"
      data-codex-voice-settings-panel="true"
    >
      <div className="flex min-h-0 flex-1 flex-col @min-[900px]/voice-settings:flex-row">
        <aside className="flex-none border-b border-[var(--glass-border-subtle)] @min-[900px]/voice-settings:w-64 @min-[900px]/voice-settings:border-r @min-[900px]/voice-settings:border-b-0 @min-[900px]/voice-settings:px-4 @min-[900px]/voice-settings:py-5">
          <div className="flex items-center gap-1 px-2 py-1.5 @min-[900px]/voice-settings:flex-col @min-[900px]/voice-settings:items-start @min-[900px]/voice-settings:gap-4 @min-[900px]/voice-settings:p-0">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={onClose}
              aria-label={t("codexVoiceBackToConversation")}
            >
              <ChevronLeftIcon size={16} />
              <span className="hidden @min-[900px]/voice-settings:inline">
                {t("codexVoiceBackToConversation")}
              </span>
            </Button>
            <h3
              id="codex-voice-settings-title"
              className="text-sm font-semibold @min-[900px]/voice-settings:px-2 @min-[900px]/voice-settings:text-[15px]"
            >
              {t("codexVoiceSettings")}
            </h3>
          </div>
          <div
            className="grid grid-cols-3 px-2 @min-[900px]/voice-settings:mt-5 @min-[900px]/voice-settings:flex @min-[900px]/voice-settings:flex-col @min-[900px]/voice-settings:gap-1 @min-[900px]/voice-settings:px-0"
            role="tablist"
            aria-label={t("codexVoiceSettings")}
          >
            {SETTINGS_SECTIONS.map((candidate) => {
              const selected = section === candidate;
              const Icon = SectionIcon[candidate];
              return (
                <button
                  key={candidate}
                  ref={(node) => {
                    if (node) tabRefs.current[candidate] = node;
                  }}
                  type="button"
                  role="tab"
                  id={`codex-voice-settings-${candidate}-tab`}
                  aria-controls={`codex-voice-settings-${candidate}-panel`}
                  aria-selected={selected}
                  tabIndex={selected ? 0 : -1}
                  className={`-mb-px flex min-h-11 min-w-0 items-center justify-center gap-2.5 border-b-2 px-1 text-xs font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] @min-[900px]/voice-settings:mb-0 @min-[900px]/voice-settings:justify-start @min-[900px]/voice-settings:rounded-lg @min-[900px]/voice-settings:border @min-[900px]/voice-settings:px-3 @min-[900px]/voice-settings:text-sm ${selected ? "border-[var(--color-accent-primary)] font-semibold text-[var(--color-text-primary)] @min-[900px]/voice-settings:border-[var(--glass-border-subtle)] @min-[900px]/voice-settings:bg-[var(--color-bg-secondary)]" : "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"}`}
                  onClick={() => setSection(candidate)}
                  onKeyDown={(event) => selectAdjacentSection(event, candidate)}
                >
                  <Icon size={17} className="hidden flex-none @min-[900px]/voice-settings:block" />
                  <span className="truncate">{sectionLabel(candidate)}</span>
                </button>
              );
            })}
          </div>
        </aside>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-[var(--color-bg-secondary)]">
          <div
            id="codex-voice-settings-audio-panel"
            role="tabpanel"
            className={`${SETTINGS_COLUMN} space-y-4 py-4 @min-[900px]/voice-settings:space-y-5 @min-[900px]/voice-settings:py-10`}
            aria-labelledby="codex-voice-settings-audio-tab"
            hidden={section !== "audio"}
          >
            <CodexVoiceSettingsHeading>{sectionLabel("audio")}</CodexVoiceSettingsHeading>
            <CodexVoiceAudioSettings
              active={active && section === "audio"}
              controller={controller}
            />
            <CodexVoicePreferenceFields preferences={preferences} section="audio" />
          </div>
          <div
            id="codex-voice-settings-notifications-panel"
            role="tabpanel"
            className={`${SETTINGS_COLUMN} space-y-4 py-4 @min-[900px]/voice-settings:space-y-5 @min-[900px]/voice-settings:py-10`}
            aria-labelledby="codex-voice-settings-notifications-tab"
            hidden={section !== "notifications"}
          >
            <CodexVoiceSettingsHeading>{sectionLabel("notifications")}</CodexVoiceSettingsHeading>
            <CodexVoicePreferenceFields preferences={preferences} section="notifications" />
          </div>
          <div
            id="codex-voice-settings-analyst-panel"
            role="tabpanel"
            className="flex min-h-full w-full flex-col"
            aria-labelledby="codex-voice-settings-analyst-tab"
            hidden={section !== "analyst"}
          >
            <CodexVoiceAnalystSettings
              active={active && section === "analyst"}
              controller={controller}
              heading={sectionLabel("analyst")}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
