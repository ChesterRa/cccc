import type { TFunction } from "i18next";
import { classNames } from "../../../utils/classNames";
import type { VoiceSecretaryCaptureMode } from "../VoiceSecretaryComposerControl";

type VoiceSecretaryRequestPanelProps = {
  actionBusy: string;
  captureMode: VoiceSecretaryCaptureMode;
  documentInstruction: string;
  isDark: boolean;
  panelRequestButtonLabel: string;
  panelRequestCollapsed: boolean;
  panelRequestCollapsedHint: string;
  panelRequestPlaceholder: string;
  panelRequestTitle: string;
  t: TFunction;
  onDocumentInstructionChange: (value: string) => void;
  onSendPanelRequest: () => void;
};

export function VoiceSecretaryRequestPanel({
  actionBusy,
  captureMode,
  documentInstruction,
  isDark,
  panelRequestButtonLabel,
  panelRequestCollapsed,
  panelRequestCollapsedHint,
  panelRequestPlaceholder,
  panelRequestTitle,
  t,
  onDocumentInstructionChange,
  onSendPanelRequest,
}: VoiceSecretaryRequestPanelProps) {
  return (
    <aside
      className={classNames(
        "flex min-h-0 flex-col gap-4 rounded-[26px] border p-3.5",
        isDark ? "border-white/10 bg-white/[0.035]" : "border-black/10 bg-[rgb(250,250,250)]",
      )}
    >
      <div
        className={classNames(
          "shrink-0 rounded-2xl border p-3 transition-[max-height,opacity] duration-200",
          panelRequestCollapsed ? "max-h-none" : "max-h-[18rem]",
          isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
        )}
      >
        <div className="flex items-start justify-between gap-2">
          <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
            {panelRequestTitle}
          </div>
          {panelRequestCollapsed ? (
            <span className={classNames(
              "shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold",
              isDark ? "bg-rose-400/12 text-rose-100" : "bg-rose-50 text-rose-700",
            )}>
              {t("voiceSecretaryRecording", { defaultValue: "Recording" })}
            </span>
          ) : null}
        </div>
        {panelRequestCollapsed ? (
          <div className="mt-2 text-[11px] leading-4 text-[var(--color-text-muted)]">
            {panelRequestCollapsedHint}
          </div>
        ) : captureMode === "prompt" ? (
          <div
            className={classNames(
              "mt-3 rounded-2xl border px-3 py-2 text-xs leading-5",
              isDark ? "border-white/10 bg-white/[0.04] text-slate-300" : "border-black/10 bg-white text-gray-700",
            )}
          >
            {panelRequestPlaceholder}
          </div>
        ) : (
          <textarea
            value={documentInstruction}
            onChange={(event) => onDocumentInstructionChange(event.target.value)}
            placeholder={panelRequestPlaceholder}
            className={classNames(
              "mt-3 min-h-[96px] w-full resize-y rounded-2xl border px-3 py-2 text-xs leading-5 outline-none transition-colors",
              isDark
                ? "border-white/10 bg-white/[0.06] text-slate-100 placeholder:text-slate-500 focus:border-white/30"
                : "border-black/10 bg-white text-gray-900 placeholder:text-gray-400 focus:border-black/25",
            )}
          />
        )}
        {!panelRequestCollapsed && captureMode !== "prompt" ? (
          <button
            type="button"
            className={classNames(
              "mt-3 w-full rounded-2xl border px-3 py-2.5 text-xs font-semibold transition-colors disabled:opacity-60",
              isDark
                ? "border-white bg-white text-[rgb(20,20,22)] hover:bg-white/90"
                : "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white hover:bg-black",
            )}
            onClick={onSendPanelRequest}
            disabled={!!actionBusy || !documentInstruction.trim()}
          >
            {panelRequestButtonLabel}
          </button>
        ) : null}
      </div>
    </aside>
  );
}
