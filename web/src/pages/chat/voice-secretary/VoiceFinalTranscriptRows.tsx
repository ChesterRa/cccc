import type { VoiceTranscriptItem } from "./voiceStreamModel";
import { classNames } from "../../../utils/classNames";
import { stripUncertainSpeakerPrefix } from "./voiceComposerUtils";

export function VoiceFinalTranscriptRows({
  transcriptRows,
  isDark,
  normalizeTranscriptText,
  formatTime,
  formatFullTime,
}: {
  transcriptRows: VoiceTranscriptItem[];
  isDark: boolean;
  normalizeTranscriptText: (value: string) => string;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
}) {
  return (
    <>
      {" "}
      {transcriptRows.map((item) => {
        const itemText = normalizeTranscriptText(stripUncertainSpeakerPrefix(item.text));
        const timeLabel = formatTime(item.updatedAt);
        const fullTimeLabel = formatFullTime(item.updatedAt);
        const sourceLabel = String(item.sourceLabel || "").trim();
        const sourceDetail = String(item.sourceDetail || "").trim();
        const rawSpeakerLabel = String(item.speakerLabel || "").trim();
        const speakerLabel = /^Speaker\s*\?$/i.test(rawSpeakerLabel) ? "" : rawSpeakerLabel;
        return (
          <div
            key={item.id}
            className={classNames(
              "rounded-lg border px-2 py-1.5",
              isDark ? "border-white/10 bg-white/[0.04]" : "border-black/[0.08] bg-white",
            )}
          >
            {speakerLabel ? (
              <div
                className={classNames(
                  "mb-0.5 text-xs font-semibold",
                  isDark ? "text-sky-100" : "text-sky-800",
                )}
              >
                {speakerLabel}
              </div>
            ) : null}
            {itemText ? (
              <div
                className={classNames(
                  "whitespace-pre-wrap break-words text-sm leading-5",
                  isDark ? "text-slate-100" : "text-gray-900",
                )}
              >
                {itemText}
              </div>
            ) : null}
            {sourceLabel || sourceDetail || timeLabel ? (
              <div className="mt-1 flex min-w-0 items-center gap-2">
                {sourceDetail ? (
                  <span className="min-w-0 flex-1 truncate text-xs text-[var(--color-text-muted)]">
                    {sourceDetail}
                  </span>
                ) : null}
                {sourceLabel ? (
                  <span
                    className={classNames(
                      "ml-auto shrink-0 rounded-full px-1.5 py-0.5 text-xs font-semibold",
                      isDark
                        ? "bg-emerald-300/10 text-emerald-100/85"
                        : "bg-emerald-50 text-emerald-800",
                    )}
                    title={sourceDetail || sourceLabel}
                  >
                    {sourceLabel}
                  </span>
                ) : null}
                {timeLabel ? (
                  <time
                    className={classNames(
                      "shrink-0 text-xs tabular-nums text-[var(--color-text-muted)]",
                      !sourceLabel && !sourceDetail && "ml-auto",
                    )}
                    dateTime={new Date(item.updatedAt).toISOString()}
                    title={fullTimeLabel}
                  >
                    {timeLabel}
                  </time>
                ) : null}
              </div>
            ) : null}
          </div>
        );
      })}{" "}
    </>
  );
}
