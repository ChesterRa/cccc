import type { VoiceTranscriptPreview } from "./voiceStreamModel";

export function VoiceLiveTranscript({
  preview,
  documentPath,
  recording,
  label,
}: {
  preview?: VoiceTranscriptPreview | null;
  documentPath: string;
  recording: boolean;
  label: string;
}) {
  if (
    !recording ||
    preview?.mode !== "document" ||
    preview.documentPath !== documentPath ||
    !preview.text.trim()
  )
    return null;
  return (
    <section
      data-voice-live-transcript
      aria-label={label}
      className="rounded-lg border border-[var(--glass-panel-border)] bg-[var(--color-bg-secondary)] p-3"
    >
      <div className="mb-1 text-xs text-[var(--color-text-muted)]">{label}</div>
      <div
        aria-live="polite"
        className="whitespace-pre-wrap break-words text-sm leading-5 text-[var(--color-text-primary)]"
      >
        {preview.text}
      </div>
    </section>
  );
}
