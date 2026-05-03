import { classNames } from "../../../utils/classNames";
import type { VoiceActivityStreamItem } from "./voiceActivityStreamModel";
import type { VoiceStreamCaptureMode } from "./voiceStreamModel";

type VoiceActivityStreamCardProps = {
  item: VoiceActivityStreamItem;
  isDark: boolean;
  isLive: boolean;
  t: (key: string, opts?: Record<string, unknown>) => string;
  voiceModeLabel: (mode: VoiceStreamCaptureMode) => string;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
};

export function VoiceActivityStreamCard({
  item,
  isDark,
  isLive,
  t,
  voiceModeLabel,
  formatTime,
  formatFullTime,
}: VoiceActivityStreamCardProps) {
  const title = String(item.documentTitle || item.documentPath || "").trim();
  const timeLabel = formatTime(item.updatedAt);
  const fullTimeLabel = formatFullTime(item.updatedAt);
  return (
    <div
      className={classNames(
        "rounded-2xl border px-2.5 py-2",
        isDark ? "border-cyan-300/20 bg-cyan-400/10" : "border-cyan-200 bg-cyan-50/70",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span
          className={classNames(
            "rounded-full px-2 py-0.5 text-[10px] font-semibold",
            isDark ? "bg-cyan-300/15 text-cyan-100" : "bg-white text-cyan-800",
          )}
        >
          {isLive
            ? t("voiceSecretaryTranscriptLive", { defaultValue: "Live" })
            : t("voiceSecretaryTranscriptHeard", { defaultValue: "Heard" })}
        </span>
        <span className="flex min-w-0 items-center gap-1.5 text-[10px] text-[var(--color-text-muted)]">
          <span className="min-w-0 truncate">{voiceModeLabel(item.mode)}</span>
          {timeLabel ? (
            <time
              className="shrink-0 tabular-nums"
              dateTime={new Date(item.updatedAt).toISOString()}
              title={fullTimeLabel}
            >
              {timeLabel}
            </time>
          ) : null}
        </span>
      </div>
      <div
        className={classNames(
          "mt-1.5 whitespace-pre-wrap break-words text-[11px] leading-4",
          isDark ? "text-cyan-50" : "text-cyan-950",
        )}
      >
        {item.text}
      </div>
      {title ? (
        <div className="mt-1 truncate text-[10px] text-[var(--color-text-muted)]">
          {title}
        </div>
      ) : null}
    </div>
  );
}
