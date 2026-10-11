// TranscriptTab configures terminal transcript visibility and viewing.
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Actor } from "../../../types";
import { SelectCombobox } from "../../SelectCombobox";
import { dangerButtonClass, inputClass, secondaryButtonClass } from "./types";
import {
  SettingsCard,
  SettingsNumberInput,
  SettingsPage,
  SettingsRow,
  SettingsSaveBar,
  SettingsSwitch,
} from "./SettingsKit";

interface TranscriptTabProps {
  isDark: boolean;
  busy: boolean;
  groupId?: string;
  devActors: Actor[];
  // Policy settings
  terminalVisibility: "off" | "foreman" | "all";
  setTerminalVisibility: (v: "off" | "foreman" | "all") => void;
  terminalNotifyTail: boolean;
  setTerminalNotifyTail: (v: boolean) => void;
  terminalNotifyLines: number;
  setTerminalNotifyLines: (v: number) => void;
  onSaveTranscriptSettings: () => void;
  // Tail viewer
  tailActorId: string;
  setTailActorId: (v: string) => void;
  tailMaxChars: number;
  setTailMaxChars: (v: number) => void;
  tailStripAnsi: boolean;
  setTailStripAnsi: (v: boolean) => void;
  tailCompact: boolean;
  setTailCompact: (v: boolean) => void;
  tailText: string;
  tailHint: string;
  tailErr: string;
  tailBusy: boolean;
  tailCopyInfo: string;
  onLoadTail: () => void;
  onCopyTail: (lines: number) => void;
  onClearTail: () => void;
}

export function TranscriptTab({
  busy,
  groupId,
  devActors,
  terminalVisibility,
  setTerminalVisibility,
  terminalNotifyTail,
  setTerminalNotifyTail,
  terminalNotifyLines,
  setTerminalNotifyLines,
  onSaveTranscriptSettings,
  tailActorId,
  setTailActorId,
  tailMaxChars,
  setTailMaxChars,
  tailStripAnsi,
  setTailStripAnsi,
  tailCompact,
  setTailCompact,
  tailText,
  tailHint,
  tailErr,
  tailBusy,
  tailCopyInfo,
  onLoadTail,
  onCopyTail,
  onClearTail,
}: TranscriptTabProps) {
  const { t } = useTranslation("settings");
  const ids = { visibility: useId(), notifyTail: useId(), notifyLines: useId(), maxChars: useId() };
  const ghost = secondaryButtonClass("sm");

  return (
    <SettingsPage
      title={t("transcript.title")}
      description={t("transcript.description")}
      footer={
        <SettingsSaveBar
          label={t("transcript.saveTranscript")}
          busy={busy}
          onSave={onSaveTranscriptSettings}
        />
      }
    >
      <SettingsCard title={t("transcript.policy")}>
        <SettingsRow
          label={t("transcript.visibilityLabel")}
          help={t("transcript.visibilityTip")}
          labelId={ids.visibility}
        >
          <div className="w-full max-w-[360px]">
            <SelectCombobox
              items={[
                { value: "off", label: t("transcript.visibilityOff") },
                { value: "foreman", label: t("transcript.visibilityForeman") },
                { value: "all", label: t("transcript.visibilityAll") },
              ]}
              value={terminalVisibility}
              onChange={(value) => {
                if (value === "off" || value === "foreman" || value === "all")
                  setTerminalVisibility(value);
              }}
              ariaLabel={t("transcript.visibilityLabel")}
              className={inputClass()}
            />
          </div>
        </SettingsRow>
        <SettingsRow label={t("transcript.includeTail")} labelId={ids.notifyTail}>
          <SettingsSwitch
            checked={terminalNotifyTail}
            onChange={setTerminalNotifyTail}
            labelledBy={ids.notifyTail}
          />
        </SettingsRow>
        <SettingsRow label={t("transcript.notificationLines")} htmlFor={ids.notifyLines}>
          <SettingsNumberInput
            id={ids.notifyLines}
            value={terminalNotifyLines}
            min={1}
            max={80}
            disabled={!terminalNotifyTail}
            onChange={(value) => setTerminalNotifyLines(value || 20)}
          />
        </SettingsRow>
      </SettingsCard>

      <SettingsCard
        title={t("transcript.tailViewer")}
        description={t("transcript.tailViewerHint")}
        actions={
          <>
            <button
              type="button"
              onClick={onLoadTail}
              disabled={!groupId || !tailActorId || tailBusy}
              className={ghost}
            >
              {tailBusy ? t("common:loading") : t("transcript.refresh")}
            </button>
            <button
              type="button"
              onClick={() => onCopyTail(50)}
              disabled={!tailText.trim()}
              className={ghost}
            >
              {t("transcript.copyLast50")}
            </button>
          </>
        }
      >
        <div className="flex flex-col gap-3 px-5 py-4">
          <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
            {/* 13rem when there is room; narrower when the card is (large text, phones). */}
            <div className="flex min-w-0 max-w-full items-center gap-2 text-[13px]">
              <span aria-hidden="true">{t("transcript.actor")}</span>
              <span className="w-52 min-w-0">
                <SelectCombobox
                  items={
                    devActors.length
                      ? devActors.map((a) => ({
                          value: a.id,
                          label: `${a.id}${a.role ? ` (${a.role})` : ""}`,
                        }))
                      : [{ value: "", label: t("transcript.noActors"), disabled: true }]
                  }
                  value={tailActorId}
                  onChange={setTailActorId}
                  ariaLabel={t("transcript.actor")}
                  className={inputClass()}
                  searchable
                />
              </span>
            </div>
            <label className="flex items-center gap-2 text-[13px]" htmlFor={ids.maxChars}>
              {t("transcript.maxChars")}
              <SettingsNumberInput
                id={ids.maxChars}
                value={tailMaxChars}
                min={1000}
                max={200000}
                onChange={(value) => setTailMaxChars(value || 8000)}
              />
            </label>
            <label className="inline-flex items-center gap-2 text-[13px] text-[var(--color-text-secondary)]">
              <input
                type="checkbox"
                checked={tailStripAnsi}
                onChange={(e) => setTailStripAnsi(e.target.checked)}
                className="h-4 w-4"
              />
              {t("transcript.stripAnsi")}
            </label>
            <label className="inline-flex items-center gap-2 text-[13px] text-[var(--color-text-secondary)]">
              <input
                type="checkbox"
                checked={tailCompact}
                disabled={!tailStripAnsi}
                onChange={(e) => setTailCompact(e.target.checked)}
                className="h-4 w-4"
              />
              {t("transcript.compactFrames")}
            </label>
          </div>
          {tailCopyInfo ? (
            <div className="text-xs text-emerald-600 dark:text-emerald-400">{tailCopyInfo}</div>
          ) : null}
          {tailErr ? (
            <div className="text-xs text-rose-600 dark:text-rose-400">{tailErr}</div>
          ) : null}
          {tailHint && !tailErr ? (
            <div className="text-xs text-[var(--color-text-muted)]">{tailHint}</div>
          ) : null}
          <pre className="m-0 max-h-[420px] min-h-[180px] overflow-auto rounded-[10px] bg-zinc-900 p-3.5 font-mono text-xs leading-6 whitespace-pre text-zinc-200">
            <code>{tailText || "\u2014"}</code>
          </pre>
        </div>
      </SettingsCard>

      <SettingsCard
        tone="danger"
        title={t("transcript.clearTruncate")}
        actions={
          <button
            type="button"
            onClick={onClearTail}
            disabled={!groupId || !tailActorId || tailBusy}
            className={dangerButtonClass("sm")}
          >
            {t("transcript.clearTruncate")}
          </button>
        }
      />
    </SettingsPage>
  );
}
