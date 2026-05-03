export const VOICE_AUDIO_LEVEL_BARS = 9;

const NOISE_FLOOR = 0.012;
const SPEECH_CEILING = 0.18;

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value));
}

function normalizeRms(rms: number): number {
  const normalized = clamp01((rms - NOISE_FLOOR) / (SPEECH_CEILING - NOISE_FLOOR));
  return Math.sqrt(normalized);
}

export function createSilentVoiceAudioLevels(barCount = VOICE_AUDIO_LEVEL_BARS): number[] {
  return Array.from({ length: Math.max(1, barCount) }, () => 0);
}

export function computeVoiceAudioLevels(samples: Float32Array, barCount = VOICE_AUDIO_LEVEL_BARS): number[] {
  const count = Math.max(1, barCount);
  if (!samples.length) return createSilentVoiceAudioLevels(count);
  const chunkSize = Math.max(1, Math.floor(samples.length / count));
  return Array.from({ length: count }, (_, index) => {
    const start = index * chunkSize;
    const end = index === count - 1 ? samples.length : Math.min(samples.length, start + chunkSize);
    if (start >= samples.length || end <= start) return 0;
    let sumSquares = 0;
    for (let sampleIndex = start; sampleIndex < end; sampleIndex += 1) {
      const sample = samples[sampleIndex] || 0;
      sumSquares += sample * sample;
    }
    return normalizeRms(Math.sqrt(sumSquares / (end - start)));
  });
}

export function smoothVoiceAudioLevels(previous: number[], next: number[], weight = 0.42): number[] {
  const count = Math.max(previous.length, next.length, VOICE_AUDIO_LEVEL_BARS);
  const nextWeight = clamp01(weight);
  const previousWeight = 1 - nextWeight;
  return Array.from({ length: count }, (_, index) => {
    const prevValue = previous[index] || 0;
    const nextValue = next[index] || 0;
    return clamp01(prevValue * previousWeight + nextValue * nextWeight);
  });
}
