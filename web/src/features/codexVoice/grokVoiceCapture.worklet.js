// AudioWorklet runs at the actual device rate. Preserve fractional sample
// boundaries across render quanta and transfer 20ms PCM16/24kHz frames.
class GrokVoiceCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.ratio = sampleRate / 24000;
    this.weight = 0;
    this.sum = 0;
    this.frame = new Int16Array(480);
    this.offset = 0;
  }
  process(inputs) {
    const input = inputs[0]?.[0];
    if (!input) return true;
    for (const sample of input) {
      let remaining = 1;
      while (remaining > 1e-8) {
        const take = Math.min(remaining, this.ratio - this.weight);
        this.sum += sample * take;
        this.weight += take;
        remaining -= take;
        if (this.weight + 1e-8 >= this.ratio) {
          const value = Math.max(-1, Math.min(1, this.sum / this.ratio));
          this.frame[this.offset++] = Math.round(value * (value < 0 ? 32768 : 32767));
          this.weight = 0;
          this.sum = 0;
          if (this.offset === this.frame.length) {
            this.port.postMessage(this.frame.buffer, [this.frame.buffer]);
            this.frame = new Int16Array(480);
            this.offset = 0;
          }
        }
      }
    }
    return true;
  }
}
registerProcessor("cccc-grok-voice-capture", GrokVoiceCapture);
