export class Pcm16Resampler {
  private static readonly outputSampleRate = 16_000;
  private readonly inputSampleRate: number;
  private carry: Float32Array<ArrayBufferLike> = new Float32Array(0);
  private phaseTicks = 0;

  constructor(inputSampleRate: number) {
    this.inputSampleRate = Math.max(1, Math.round(Number(inputSampleRate) || 48_000));
  }

  push(input: Float32Array): Uint8Array {
    if (!input.length) return new Uint8Array(0);
    const samples = this.carry.length ? new Float32Array(this.carry.length + input.length) : input;
    if (this.carry.length) {
      samples.set(this.carry, 0);
      samples.set(input, this.carry.length);
    }
    // Use an integer clock so 44.1 kHz keeps its exact phase across browser callbacks.
    const sampleTicks = Pcm16Resampler.outputSampleRate;
    const availableTicks = samples.length * sampleTicks - this.phaseTicks;
    const outputLength = Math.max(0, Math.floor(availableTicks / this.inputSampleRate));
    if (outputLength <= 0) {
      this.carry = samples;
      return new Uint8Array(0);
    }
    const output = new Int16Array(outputLength);
    for (let index = 0; index < outputLength; index += 1) {
      const start = this.phaseTicks + index * this.inputSampleRate;
      const end = start + this.inputSampleRate;
      const startIndex = Math.floor(start / sampleTicks);
      const endIndex = Math.max(startIndex + 1, Math.ceil(end / sampleTicks));
      let total = 0;
      let totalWeight = 0;
      for (let sourceIndex = startIndex; sourceIndex < endIndex; sourceIndex += 1) {
        const sampleStart = Math.max(start, sourceIndex * sampleTicks);
        const sampleEnd = Math.min(end, (sourceIndex + 1) * sampleTicks);
        const weight = Math.max(0, sampleEnd - sampleStart);
        if (weight > 0) {
          total += (samples[sourceIndex] || 0) * weight;
          totalWeight += weight;
        }
      }
      const averaged = totalWeight > 0 ? total / totalWeight : 0;
      // Browser capture may already apply AGC; encode at unity gain to avoid double amplification.
      const sample = Math.max(-1, Math.min(1, averaged));
      output[index] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    }
    const nextStart = this.phaseTicks + outputLength * this.inputSampleRate;
    const consumedSamples = Math.min(samples.length, Math.floor(nextStart / sampleTicks));
    this.phaseTicks = nextStart - consumedSamples * sampleTicks;
    this.carry =
      consumedSamples < samples.length ? samples.slice(consumedSamples) : new Float32Array(0);
    return new Uint8Array(output.buffer);
  }
}
