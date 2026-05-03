import { classNames } from "../../../utils/classNames";
import { createSilentVoiceAudioLevels, VOICE_AUDIO_LEVEL_BARS } from "./voiceAudioLevel";

type VoiceTranscriptRecordingIndicatorProps = {
  isDark: boolean;
  label: string;
  compact?: boolean;
  levels?: number[];
};

const BAR_HEIGHTS = [16, 28, 20, 34, 24, 30, 18, 26, 14];
const silentLevels = createSilentVoiceAudioLevels(VOICE_AUDIO_LEVEL_BARS);

export function VoiceTranscriptRecordingIndicator({
  isDark,
  label,
  compact = false,
  levels = silentLevels,
}: VoiceTranscriptRecordingIndicatorProps) {
  const displayLevels = levels.length ? levels : silentLevels;
  return (
    <div
      className={classNames(
        "flex items-center justify-center rounded-2xl border",
        compact ? "gap-2 px-3 py-2" : "min-h-[280px] flex-col gap-4 px-4 py-8",
        isDark ? "border-white/10 bg-white/[0.035]" : "border-black/[0.08] bg-white/75",
      )}
      role="status"
      aria-live="polite"
    >
      <div
        className={classNames(
          "flex items-center justify-center gap-1.5",
          compact ? "h-8" : "h-14",
        )}
        aria-hidden="true"
      >
        {BAR_HEIGHTS.map((height, index) => {
          const level = Math.max(0, Math.min(1, Number(displayLevels[index]) || 0));
          const restingHeight = compact ? 8 : 12;
          const maxHeight = compact ? Math.max(8, Math.round(height * 0.58)) : height;
          const visibleHeight = Math.round(restingHeight + (maxHeight - restingHeight) * level);
          return (
            <span
              key={`${height}-${index}`}
              className={classNames(
                "block w-1.5 rounded-full transition-[height,opacity] duration-75 motion-reduce:transition-none",
                isDark ? "bg-emerald-200/85" : "bg-emerald-700/85",
              )}
              style={{
                height: visibleHeight,
                opacity: 0.36 + level * 0.5,
              }}
            />
          );
        })}
      </div>
      <div
        className={classNames(
          "font-semibold",
          compact ? "text-xs" : "text-sm",
          isDark ? "text-slate-200" : "text-gray-700",
        )}
      >
        {label}
      </div>
    </div>
  );
}
