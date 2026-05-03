export type VoiceFinalTranscriptSegment = {
  text: string;
  startMs?: number;
  endMs?: number;
};

export type VoiceFinalTranscriptUtterance = VoiceFinalTranscriptSegment;

const DEFAULT_MAX_GAP_MS = 2200;
const DEFAULT_MIN_CHARS = 28;
const DEFAULT_MAX_CHARS = 180;
const DEFAULT_MAX_DURATION_MS = 45000;

function cleanSegmentText(value: string): string {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function hasTerminalPunctuation(value: string): boolean {
  return /[。！？!?；;：:]$/.test(String(value || "").trim());
}

function joinTranscriptText(left: string, right: string): string {
  const lhs = cleanSegmentText(left);
  const rhs = cleanSegmentText(right);
  if (!lhs) return rhs;
  if (!rhs) return lhs;
  return `${lhs} ${rhs}`.replace(/\s+/g, " ").trim();
}

function shouldStartNewUtterance(
  current: VoiceFinalTranscriptUtterance,
  next: VoiceFinalTranscriptSegment,
  opts: {
    maxGapMs: number;
    minChars: number;
    maxChars: number;
    maxDurationMs: number;
  },
): boolean {
  const { maxGapMs, minChars, maxChars, maxDurationMs } = opts;
  const currentText = cleanSegmentText(current.text);
  const nextText = cleanSegmentText(next.text);
  if (!currentText || !nextText) return false;
  const gapMs = Number.isFinite(Number(current.endMs)) && Number.isFinite(Number(next.startMs))
    ? Number(next.startMs) - Number(current.endMs)
    : 0;
  const durationMs = Number.isFinite(Number(current.startMs)) && Number.isFinite(Number(next.endMs))
    ? Number(next.endMs) - Number(current.startMs)
    : 0;
  if (currentText.length < minChars) return false;
  if (gapMs > maxGapMs) return true;
  if (hasTerminalPunctuation(currentText) && currentText.length >= minChars) return true;
  if (joinTranscriptText(currentText, nextText).length > maxChars) return true;
  return durationMs > maxDurationMs;
}

export function assembleVoiceFinalTranscriptSegments(
  segments: VoiceFinalTranscriptSegment[],
  opts: {
    maxGapMs?: number;
    minChars?: number;
    maxChars?: number;
    maxDurationMs?: number;
  } = {},
): VoiceFinalTranscriptUtterance[] {
  const maxGapMs = Math.max(0, Number(opts.maxGapMs ?? DEFAULT_MAX_GAP_MS));
  const minChars = Math.max(1, Number(opts.minChars ?? DEFAULT_MIN_CHARS));
  const maxChars = Math.max(minChars, Number(opts.maxChars ?? DEFAULT_MAX_CHARS));
  const maxDurationMs = Math.max(1, Number(opts.maxDurationMs ?? DEFAULT_MAX_DURATION_MS));
  const utterances: VoiceFinalTranscriptUtterance[] = [];
  for (const rawSegment of segments) {
    const text = cleanSegmentText(rawSegment.text);
    if (!text) continue;
    const next: VoiceFinalTranscriptSegment = {
      text,
      startMs: rawSegment.startMs,
      endMs: rawSegment.endMs,
    };
    const current = utterances[utterances.length - 1];
    if (
      !current
      || shouldStartNewUtterance(current, next, { maxGapMs, minChars, maxChars, maxDurationMs })
    ) {
      utterances.push({ ...next });
      continue;
    }
    current.text = joinTranscriptText(current.text, next.text);
    if (!Number.isFinite(Number(current.startMs)) && Number.isFinite(Number(next.startMs))) {
      current.startMs = next.startMs;
    }
    if (Number.isFinite(Number(next.endMs))) {
      current.endMs = next.endMs;
    }
  }
  return utterances;
}
