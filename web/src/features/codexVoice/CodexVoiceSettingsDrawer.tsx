import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { SettingsIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { useModalA11y } from "../../hooks/useModalA11y";
import { CodexVoiceAnalystSettings } from "./CodexVoiceAnalystSettings";
import { CodexVoiceAudioSettings } from "./CodexVoiceAudioSettings";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

type SettingsSection = "audio" | "analyst";

type Props = { active: boolean; controller: CodexVoiceSessionController; onClose: () => void };

const SETTINGS_SECTIONS: SettingsSection[] = ["audio", "analyst"];

export function CodexVoiceSettingsDrawer({ active, controller, onClose }: Props) {
  const { t } = useTranslation("modals");
  const [section, setSection] = useState<SettingsSection>("audio");
  const tabRefs = useRef<Record<SettingsSection, HTMLButtonElement | null>>({
    audio: null,
    analyst: null,
  });
  const { modalRef } = useModalA11y(true, onClose);

  useEffect(() => {
    const frame = requestAnimationFrame(() => tabRefs.current.audio?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);

  const selectAdjacentSection = (
    event: KeyboardEvent<HTMLButtonElement>,
    current: SettingsSection,
  ) => {
    const currentIndex = SETTINGS_SECTIONS.indexOf(current);
    let nextIndex: number | undefined;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      nextIndex = (currentIndex + 1) % SETTINGS_SECTIONS.length;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      nextIndex = (currentIndex - 1 + SETTINGS_SECTIONS.length) % SETTINGS_SECTIONS.length;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = SETTINGS_SECTIONS.length - 1;
    }
    if (nextIndex === undefined) return;
    event.preventDefault();
    const next = SETTINGS_SECTIONS[nextIndex];
    setSection(next);
    tabRefs.current[next]?.focus();
  };

  return (
    <div
      className="absolute inset-0 z-30 flex items-start"
      data-codex-voice-settings-overlay="true"
    >
      <button
        type="button"
        tabIndex={-1}
        className="absolute inset-0 cursor-default bg-black/20 dark:bg-black/50"
        aria-label={t("codexVoiceSettingsClose")}
        onClick={onClose}
      />

      <section
        ref={modalRef}
        id="codex-voice-settings-drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="codex-voice-settings-title"
        className="relative z-[1] flex max-h-[calc(100%-0.75rem)] w-full flex-col overflow-hidden rounded-b-2xl border-x border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] shadow-[0_24px_55px_-28px_rgba(0,0,0,0.7)] animate-codex-voice-drawer-in motion-reduce:animate-none sm:max-h-[82%] sm:rounded-b-[24px]"
        data-codex-voice-settings-drawer="true"
      >
        <div className="flex flex-none items-start justify-between gap-4 border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-5 py-3.5 sm:px-6">
          <div className="flex min-w-0 items-start gap-3">
            <span className="mt-0.5 flex h-8 w-8 flex-none items-center justify-center rounded-xl bg-[var(--glass-tab-bg-active)] text-[var(--color-text-secondary)]">
              <SettingsIcon size={16} aria-hidden="true" />
            </span>
            <div className="min-w-0">
              <h3
                id="codex-voice-settings-title"
                className="text-sm font-semibold text-[var(--color-text-primary)]"
              >
                {t("codexVoiceSettings")}
              </h3>
              <p className="mt-0.5 text-xs leading-5 text-[var(--color-text-muted)]">
                {t("codexVoiceSettingsHint")}
              </p>
            </div>
          </div>
          <Button type="button" variant="ghost" size="sm" onClick={onClose}>
            {t("codexVoiceSettingsClose")}
          </Button>
        </div>

        <div
          className="flex flex-none gap-1 border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-5 pt-1 sm:px-6"
          role="tablist"
          aria-label={t("codexVoiceSettings")}
        >
          {SETTINGS_SECTIONS.map((candidate) => {
            const selected = section === candidate;
            return (
              <button
                key={candidate}
                ref={(node) => {
                  tabRefs.current[candidate] = node;
                }}
                type="button"
                role="tab"
                id={`codex-voice-settings-${candidate}-tab`}
                aria-controls={`codex-voice-settings-${candidate}-panel`}
                aria-selected={selected}
                tabIndex={selected ? 0 : -1}
                data-settings-initial-focus={candidate === "audio" ? "true" : undefined}
                className={`border-b-2 px-3 py-2.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 ${
                  selected
                    ? "border-[var(--color-accent-primary)] text-[var(--color-text-primary)]"
                    : "border-transparent text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]"
                }`}
                onClick={() => setSection(candidate)}
                onKeyDown={(event) => selectAdjacentSection(event, candidate)}
              >
                {t(
                  candidate === "audio" ? "codexVoiceSettingsVoiceAudio" : "codexVoiceAnalystTitle",
                )}
              </button>
            );
          })}
        </div>

        <div className="min-h-0 overflow-y-auto overscroll-contain bg-[var(--color-bg-primary)]">
          <div
            id="codex-voice-settings-audio-panel"
            role="tabpanel"
            aria-labelledby="codex-voice-settings-audio-tab"
            hidden={section !== "audio"}
          >
            <CodexVoiceAudioSettings
              active={active && section === "audio"}
              controller={controller}
            />
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
    </div>
  );
}
