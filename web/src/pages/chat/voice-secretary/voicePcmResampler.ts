export class Pcm16Resampler {
  private readonly ratio: number;
  private carry = new Float32Array(0);
  private gain = 1.8;

  constructor(inputSampleRate: number) {
    const sourceRate = Math.max(1, Number(inputSampleRate) || 48000);
    this.ratio = sourceRate / 16000;
  }

  push(input: Float32Array): Uint8Array {
    if (!input.length) return new Uint8Array(0);
    const samples = this.carry.length ? new Float32Array(this.carry.length + input.length) : input;
    if (this.carry.length) {
      samples.set(this.carry, 0);
      samples.set(input, this.carry.length);
    }
    const outputLength = Math.max(0, Math.floor(samples.length / this.ratio));
    const consumedSamples = Math.min(samples.length, Math.floor(outputLength * this.ratio));
    this.carry =
      consumedSamples < samples.length ? samples.slice(consumedSamples) : new Float32Array(0);
    if (outputLength <= 0) return new Uint8Array(0);
    const output = new Int16Array(outputLength);
    for (let index = 0; index < outputLength; index += 1) {
      const start = index * this.ratio;
      const end = Math.min(samples.length, (index + 1) * this.ratio);
      const startIndex = Math.floor(start);
      const endIndex = Math.max(startIndex + 1, Math.ceil(end));
      let total = 0;
      let totalWeight = 0;
      for (let sourceIndex = startIndex; sourceIndex < endIndex; sourceIndex += 1) {
        const sampleStart = Math.max(start, sourceIndex);
        const sampleEnd = Math.min(end, sourceIndex + 1);
        const weight = Math.max(0, sampleEnd - sampleStart);
        if (weight > 0) {
          total += (samples[sourceIndex] || 0) * weight;
          totalWeight += weight;
        }
      }
      const averaged = totalWeight > 0 ? total / totalWeight : 0;
      const sample = Math.max(-1, Math.min(1, averaged * this.gain));
      output[index] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    }
    return new Uint8Array(output.buffer);
  }
}
