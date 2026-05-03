// TranscriptTab configures terminal transcript visibility and viewing.
import { useTranslation } from "react-i18next";
import { Actor } from "../../../types";
import { SelectCombobox } from "../../SelectCombobox";
import {
  dangerButtonClass,
  inputClass,
  preClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

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
  isDark: _isDark,
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

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{t("transcript.title")}</h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("transcript.description")}
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspacePanelClass(_isDark)}>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("transcript.policy")}</div>

            <div className="mt-4 space-y-4">
              <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                <label className="block text-xs mb-1 text-[var(--color-text-tertiary)]">{t("transcript.visibilityLabel")}</label>
                <SelectCombobox
                  items={[
                    { value: "off", label: t("transcript.visibilityOff") },
                    { value: "foreman", label: t("transcript.visibilityForeman") },
                    { value: "all", label: t("transcript.visibilityAll") },
                  ]}
                  value={terminalVisibility}
                  onChange={(value) => {
                    const v = value;
                    if (v === "off" || v === "foreman" || v === "all") setTerminalVisibility(v);
                  }}
                  ariaLabel={t("transcript.visibilityLabel")}
                  className={inputClass()}
                />
                <div className="mt-2 text-[11px] text-[var(--color-text-muted)]">
                  {t("transcript.visibilityTip")}
                </div>
              </div>

              <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_220px]">
                <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                  <label className="inline-flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
                    <input
                      type="checkbox"
                      checked={terminalNotifyTail}
                      onChange={(e) => setTerminalNotifyTail(e.target.checked)}
                      className="h-4 w-4"
                    />
                    {t("transcript.includeTail")}
                  </label>
                </div>

                <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                  <label className="block text-xs mb-1 text-[var(--color-text-tertiary)]">{t("transcript.notificationLines")}</label>
                  <input
                    type="number"
                    value={terminalNotifyLines}
                    min={1}
                    max={80}
                    onChange={(e) => setTerminalNotifyLines(Number(e.target.value || 20))}
                    disabled={!terminalNotifyTail}
                    className={`${inputClass()} disabled:opacity-60`}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className={settingsWorkspaceActionBarClass(_isDark)}>
          <button
            onClick={onSaveTranscriptSettings}
            disabled={busy}
            className={primaryButtonClass(busy)}
          >
            {busy ? t("common:saving") : t("transcript.saveTranscript")}
          </button>
        </div>
      </div>

      <div className={settingsWorkspaceShellClass(_isDark)}>
        <div className={settingsWorkspaceHeaderClass(_isDark)}>
          <div className="min-w-0">
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">{t("transcript.tailViewer")}</div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("transcript.tailViewerHint")}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={onLoadTail}
              disabled={!groupId || !tailActorId || tailBusy}
              className={secondaryButtonClass()}
            >
              {tailBusy ? t("common:loading") : t("transcript.refresh")}
            </button>
            <button
              onClick={() => onCopyTail(50)}
              disabled={!tailText.trim()}
              className={secondaryButtonClass()}
            >
              {t("transcript.copyLast50")}
            </button>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <div className={settingsWorkspacePanelClass(_isDark)}>
              <div className="space-y-4">
                <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                  <label className="block text-xs text-[var(--color-text-tertiary)]">{t("transcript.actor")}</label>
                  <SelectCombobox
                    items={
                      devActors.length
                        ? devActors.map((a) => ({ value: a.id, label: `${a.id}${a.role ? ` (${a.role})` : ""}` }))
                        : [{ value: "", label: t("transcript.noActors"), disabled: true }]
                    }
                    value={tailActorId}
                    onChange={setTailActorId}
                    ariaLabel={t("transcript.actor")}
                    className={`${inputClass()} mt-2`}
                    searchable
                  />
                </div>

                <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_auto]">
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <label className="block text-xs mb-1 text-[var(--color-text-tertiary)]">{t("transcript.maxChars")}</label>
                    <input
                      type="number"
                      value={tailMaxChars}
                      min={1000}
                      max={200000}
                      onChange={(e) => setTailMaxChars(Number(e.target.value || 8000))}
                      className={inputClass()}
                    />
                  </div>

                  <div className="flex items-end">
                    <button
                      onClick={onClearTail}
                      disabled={!groupId || !tailActorId || tailBusy}
                      className={dangerButtonClass()}
                    >
                      {t("transcript.clearTruncate")}
                    </button>
                  </div>
                </div>

                <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <label className="inline-flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
                      <input
                        type="checkbox"
                        checked={tailStripAnsi}
                        onChange={(e) => setTailStripAnsi(e.target.checked)}
                        className="h-4 w-4"
                      />
                      {t("transcript.stripAnsi")}
                    </label>
                  </div>
                  <div className={settingsWorkspaceSoftPanelClass(_isDark)}>
                    <label className="inline-flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
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
                </div>
              </div>
            </div>

            <div className={settingsWorkspacePanelClass(_isDark)}>
              {!!tailCopyInfo && (
                <div className="mb-3 text-xs text-emerald-600 dark:text-emerald-400">{tailCopyInfo}</div>
              )}
              {tailErr && (
                <div className="mb-3 text-xs text-rose-600 dark:text-rose-400">{tailErr}</div>
              )}
              {tailHint && !tailErr && (
                <div className="mb-3 text-xs text-[var(--color-text-muted)]">{tailHint}</div>
              )}

              <pre className={`${preClass()} mt-0 max-h-[420px] overflow-y-auto`}>
                <code>{tailText || "\u2014"}</code>
              </pre>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
