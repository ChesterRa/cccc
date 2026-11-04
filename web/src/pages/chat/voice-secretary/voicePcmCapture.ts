export class VoicePcmCaptureError extends Error {
  constructor(readonly code: "unsupported" | "module_failed" | "processor_failed") {
    super(code);
    this.name = "VoicePcmCaptureError";
  }
}

type CaptureCallbacks = {
  onSamples: (samples: Float32Array) => void;
  onError: (error: VoicePcmCaptureError) => void;
  onContextState: (state: string) => void;
};

/** Owns the Web Audio nodes; the caller owns and stops the microphone track. */
export class VoicePcmCapture {
  private source: MediaStreamAudioSourceNode | null = null;
  private closed = false;
  private flushPromise: Promise<boolean> | null = null;
  private finishFlush: ((complete: boolean) => void) | null = null;
  private flushTimer: number | null = null;
  private resumeTimer: number | null = null;

  private constructor(
    private readonly context: AudioContext,
    private readonly node: AudioWorkletNode,
    private readonly callbacks: CaptureCallbacks,
  ) {
    node.port.onmessage = (event: MessageEvent<unknown>) => {
      if (this.closed || !event.data || typeof event.data !== "object") return;
      const data = event.data as { type?: unknown; buffer?: unknown };
      if (data.type === "samples") {
        if (!(data.buffer instanceof ArrayBuffer) || data.buffer.byteLength % 4 !== 0) {
          callbacks.onError(new VoicePcmCaptureError("processor_failed"));
          return;
        }
        callbacks.onSamples(new Float32Array(data.buffer));
      } else if (data.type === "flushed") {
        this.finishFlush?.(true);
      }
    };
    node.onprocessorerror = () => {
      if (this.closed) return;
      this.finishFlush?.(false);
      callbacks.onError(new VoicePcmCaptureError("processor_failed"));
    };
    context.onstatechange = () => {
      if (!this.closed) callbacks.onContextState(context.state);
    };
  }

  static async create(callbacks: CaptureCallbacks): Promise<VoicePcmCapture> {
    const AudioContextConstructor =
      window.AudioContext ||
      (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextConstructor || typeof AudioWorkletNode === "undefined") {
      throw new VoicePcmCaptureError("unsupported");
    }
    const context = new AudioContextConstructor();
    if (!context.audioWorklet) {
      void context.close().catch(() => undefined);
      throw new VoicePcmCaptureError("unsupported");
    }
    let loadTimer: number | null = null;
    try {
      await Promise.race([
        context.audioWorklet.addModule(
          new URL("./voicePcmCapture.worklet.js", import.meta.url).href,
        ),
        new Promise<never>((_resolve, reject) => {
          loadTimer = window.setTimeout(
            () => reject(new Error("audio module load timed out")),
            10_000,
          );
        }),
      ]);
      const node = new AudioWorkletNode(context, "cccc-secretary-pcm-capture", {
        channelCount: 1,
        channelCountMode: "explicit",
        outputChannelCount: [1],
      });
      return new VoicePcmCapture(context, node, callbacks);
    } catch {
      void context.close().catch(() => undefined);
      throw new VoicePcmCaptureError("module_failed");
    } finally {
      if (loadTimer !== null) window.clearTimeout(loadTimer);
    }
  }

  get sampleRate(): number {
    return this.context.sampleRate;
  }

  connect(stream: MediaStream): void {
    if (this.closed) throw new VoicePcmCaptureError("processor_failed");
    this.source = this.context.createMediaStreamSource(stream);
    this.source.connect(this.node);
    // The worklet emits silence, but needs an output connection to keep the graph running.
    this.node.connect(this.context.destination);
    const failResume = () => {
      if (!this.closed) this.callbacks.onError(new VoicePcmCaptureError("processor_failed"));
    };
    const clearResumeTimer = () => {
      if (this.resumeTimer !== null) window.clearTimeout(this.resumeTimer);
      this.resumeTimer = null;
    };
    this.resumeTimer = window.setTimeout(() => {
      this.resumeTimer = null;
      if (this.context.state !== "running") failResume();
    }, 10_000);
    void this.context
      .resume()
      .then(() => {
        clearResumeTimer();
        if (this.context.state !== "running") failResume();
      })
      .catch(() => {
        clearResumeTimer();
        failResume();
      });
  }

  /** Flushes already-processed samples. The caller must stop the microphone first. */
  flush(timeoutMs = 1_000): Promise<boolean> {
    if (this.closed) return Promise.resolve(false);
    if (this.flushPromise) return this.flushPromise;
    this.flushPromise = new Promise((resolve) => {
      this.finishFlush = (complete) => {
        if (!this.finishFlush) return;
        this.finishFlush = null;
        if (this.flushTimer !== null) window.clearTimeout(this.flushTimer);
        this.flushTimer = null;
        resolve(complete);
      };
      this.flushTimer = window.setTimeout(() => this.finishFlush?.(false), timeoutMs);
      try {
        this.node.port.postMessage({ type: "flush" });
      } catch {
        this.finishFlush(false);
      }
    });
    return this.flushPromise;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.finishFlush?.(false);
    if (this.resumeTimer !== null) window.clearTimeout(this.resumeTimer);
    this.resumeTimer = null;
    this.node.port.onmessage = null;
    this.node.onprocessorerror = null;
    this.context.onstatechange = null;
    try {
      this.source?.disconnect();
      this.node.disconnect();
      this.node.port.close();
    } catch {
      // Closing the AudioContext below remains necessary if a node was already disconnected.
    }
    void this.context.close().catch(() => undefined);
  }
}
