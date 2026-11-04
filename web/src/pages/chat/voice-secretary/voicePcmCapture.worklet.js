// Aggregate short render quanta into roughly 20 ms messages. The input block
// length is supplied by the browser and is not assumed to stay at 128 frames.
class VoiceSecretaryPcmCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frame = new Float32Array(Math.max(1, Math.round(sampleRate * 0.02)));
    this.offset = 0;
    this.stopped = false;
    this.port.onmessage = (event) => {
      if (event.data?.type !== "flush" || this.stopped) return;
      this.stopped = true;
      if (this.offset) {
        const remaining = this.frame.slice(0, this.offset);
        this.port.postMessage({ type: "samples", buffer: remaining.buffer }, [remaining.buffer]);
      }
      this.port.postMessage({ type: "flushed" });
    };
  }

  process(inputs) {
    if (this.stopped) return false;
    const input = inputs[0]?.[0];
    if (!input) return true;
    let start = 0;
    while (start < input.length) {
      const count = Math.min(input.length - start, this.frame.length - this.offset);
      this.frame.set(input.subarray(start, start + count), this.offset);
      this.offset += count;
      start += count;
      if (this.offset === this.frame.length) {
        const completed = this.frame;
        this.frame = new Float32Array(completed.length);
        this.offset = 0;
        this.port.postMessage({ type: "samples", buffer: completed.buffer }, [completed.buffer]);
      }
    }
    return true;
  }
}

registerProcessor("cccc-secretary-pcm-capture", VoiceSecretaryPcmCapture);
