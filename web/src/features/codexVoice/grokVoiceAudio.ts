import { failure } from "./codexVoiceProtocol";

const SAMPLE_RATE = 24_000;
const MAX_QUEUED_SECONDS = 120;
const PLAYBACK_BUFFER_SECONDS = 0.06;

/** Browser-owned capture and playback. Generated audio is never sent to CCCC. */
export class GrokVoiceAudio {
  private readonly context: AudioContext;
  private readonly destination: MediaStreamAudioDestinationNode;
  private capture: AudioWorkletNode | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private readonly nodes = new Map<AudioBufferSourceNode, string>();
  private nextPlaybackTime = 0;
  private closed = false;
  private readonly ready: Promise<void>;

  constructor(
    private readonly audio: HTMLAudioElement,
    private readonly onSpeaking: (speaking: boolean) => void,
    private readonly onDrained: (responseId: string) => void,
    private readonly onBlocked: (blocked: boolean) => void,
  ) {
    if (typeof AudioContext === "undefined" || typeof AudioWorkletNode === "undefined")
      throw failure("audio_worklet_unsupported");
    this.context = new AudioContext();
    this.destination = this.context.createMediaStreamDestination();
    audio.srcObject = this.destination.stream;
    // Start within the user's gesture, before provider/network awaits.
    void this.resume();
    this.ready = this.context.audioWorklet.addModule(
      new URL("./grokVoiceCapture.worklet.js", import.meta.url).href,
    );
    // A stop can win before the module load is awaited by startCapture.
    void this.ready.catch(() => undefined);
  }

  async startCapture(stream: MediaStream, onFrame: (buffer: ArrayBuffer) => void): Promise<void> {
    await this.ready;
    if (this.closed) return;
    this.source = this.context.createMediaStreamSource(stream);
    this.capture = new AudioWorkletNode(this.context, "cccc-grok-voice-capture", {
      channelCount: 1,
      channelCountMode: "explicit",
      outputChannelCount: [1],
    });
    this.capture.port.onmessage = (event: MessageEvent<unknown>) => {
      if (!this.closed && event.data instanceof ArrayBuffer) onFrame(event.data);
    };
    this.source.connect(this.capture);
    // The processor has a silent output; connect it to keep the render graph live.
    this.capture.connect(this.context.destination);
  }

  append(encoded: string, responseId: string): void {
    if (this.closed) return;
    const raw = atob(encoded);
    if (!raw.length || raw.length % 2 !== 0) throw failure("provider_audio_invalid");
    const now = this.context.currentTime;
    // Buffer only at startup or after an underrun; queued frames remain consecutive.
    const start =
      this.nextPlaybackTime > now ? this.nextPlaybackTime : now + PLAYBACK_BUFFER_SECONDS;
    if (start - now + raw.length / 2 / SAMPLE_RATE > MAX_QUEUED_SECONDS)
      throw failure("provider_audio_overflow");
    const buffer = this.context.createBuffer(1, raw.length / 2, SAMPLE_RATE);
    const samples = buffer.getChannelData(0);
    for (let i = 0; i < samples.length; i++) {
      const unsigned = raw.charCodeAt(i * 2) | (raw.charCodeAt(i * 2 + 1) << 8);
      samples[i] = (unsigned >= 32768 ? unsigned - 65536 : unsigned) / 32768;
    }
    const node = this.context.createBufferSource();
    node.buffer = buffer;
    node.connect(this.destination);
    this.nodes.set(node, responseId);
    node.onended = () => {
      if (!this.nodes.delete(node) || this.closed) return;
      node.disconnect();
      if (!this.hasResponse(responseId)) this.onDrained(responseId);
      if (this.nodes.size === 0) this.onSpeaking(false);
    };
    node.start(start);
    this.nextPlaybackTime = start + buffer.duration;
    this.onSpeaking(true);
  }

  hasResponse(responseId: string): boolean {
    return [...this.nodes.values()].includes(responseId);
  }

  isPlaying(): boolean {
    return this.nodes.size > 0;
  }

  interrupt(): void {
    const nodes = [...this.nodes.keys()];
    this.nodes.clear();
    this.nextPlaybackTime = 0;
    for (const node of nodes) {
      node.onended = null;
      try {
        node.stop();
      } catch {
        /* Already ended. */
      }
      node.disconnect();
    }
    if (!this.closed) this.onSpeaking(false);
  }

  async resume(): Promise<boolean> {
    if (this.closed) return false;
    try {
      await this.context.resume();
      if (this.closed) return false;
      await this.audio.play();
      if (this.closed) return false;
      this.onBlocked(false);
      return true;
    } catch {
      if (!this.closed) {
        this.onBlocked(true);
        // Retain queued output until the user enables audio, rather than
        // draining it into a paused HTMLAudioElement and claiming playback.
        await this.context.suspend().catch(() => undefined);
      }
      return false;
    }
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.interrupt();
    if (this.capture) this.capture.port.onmessage = null;
    this.capture?.disconnect();
    this.source?.disconnect();
    for (const track of this.destination.stream.getTracks()) track.stop();
    this.audio.pause();
    this.audio.srcObject = null;
    void this.context.close().catch(() => undefined);
  }
}
