import { classNames } from "../../../utils/classNames";

type VoiceTranscriptRecordingIndicatorProps = { isDark: boolean; label: string; compact?: boolean };

/** Recording / analyzing status text; the workspace panel's glow carries the audio reaction. */
export function VoiceTranscriptRecordingIndicator({
  isDark,
  label,
  compact = false,
}: VoiceTranscriptRecordingIndicatorProps) {
  return (
    <div
      className={classNames(
        "flex items-center justify-center border text-center",
        compact
          ? "min-h-11 rounded-xl px-3 py-2"
          : "min-h-[280px] flex-col gap-4 rounded-2xl px-4 py-8",
        isDark ? "border-white/10 bg-white/[0.035]" : "border-black/[0.08] bg-white/75",
      )}
      role="status"
      aria-live="polite"
    >
      <div
        className={classNames(
          "min-w-0 font-semibold",
          compact ? "truncate text-[11px]" : "text-sm",
          isDark ? "text-slate-200" : "text-gray-700",
        )}
        title={compact ? label : undefined}
      >
        {label}
      </div>
    </div>
  );
}
