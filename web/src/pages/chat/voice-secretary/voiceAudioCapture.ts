export const VOICE_AUDIO_CAPTURE_TIMEOUT_MS = 20_000;

export class VoiceAudioCaptureTimeoutError extends Error {
  constructor() {
    super("microphone capture request timed out");
    this.name = "VoiceAudioCaptureTimeoutError";
  }
}

export async function getUserMediaWithTimeout(
  constraints: MediaStreamConstraints,
  timeoutMs: number = VOICE_AUDIO_CAPTURE_TIMEOUT_MS,
): Promise<MediaStream> {
  let timedOut = false;
  let timeoutId: number | undefined;
  const capture = navigator.mediaDevices.getUserMedia(constraints);
  void capture.then(
    (stream) => {
      if (timedOut) stream.getTracks().forEach((track) => track.stop());
    },
    () => undefined,
  );
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = window.setTimeout(
      () => {
        timedOut = true;
        reject(new VoiceAudioCaptureTimeoutError());
      },
      Math.max(1, timeoutMs),
    );
  });
  try {
    return await Promise.race([capture, timeout]);
  } finally {
    if (timeoutId !== undefined) window.clearTimeout(timeoutId);
  }
}
