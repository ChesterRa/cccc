const NOISE_FLOOR = 0.012;
const SPEECH_CEILING = 0.18;

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value));
}

/** Speech loudness of one audio frame, 0–1; VoiceBeam applies its own gate and smoothing. */
export function computeVoiceAudioLevel(samples: Float32Array): number {
  if (!samples.length) return 0;
  let sumSquares = 0;
  for (const sample of samples) sumSquares += sample * sample;
  const rms = Math.sqrt(sumSquares / samples.length);
  return Math.sqrt(clamp01((rms - NOISE_FLOOR) / (SPEECH_CEILING - NOISE_FLOOR)));
}
