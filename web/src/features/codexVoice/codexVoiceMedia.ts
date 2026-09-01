import { failure } from "./codexVoiceProtocol";

const ICE_TIMEOUT_MS = 12_000;
const PEER_READY_TIMEOUT_MS = 15_000;

export async function captureMicrophone(inputDeviceId: string): Promise<MediaStream> {
  const audio: MediaTrackConstraints = {
    channelCount: { ideal: 1 },
    echoCancellation: { ideal: true },
    noiseSuppression: { ideal: true },
    autoGainControl: { ideal: true },
  };
  if (inputDeviceId) audio.deviceId = { exact: inputDeviceId };
  try {
    return await navigator.mediaDevices.getUserMedia({ audio });
  } catch (error) {
    if (isUnavailableAudioDevice(error)) throw failure("microphone_unavailable");
    if (error instanceof DOMException && error.name === "NotAllowedError") {
      throw failure("microphone_permission_denied");
    }
    throw failure("microphone_failed");
  }
}

export async function applyOutputDevice(
  audio: HTMLAudioElement,
  outputDeviceId: string,
): Promise<void> {
  if (!outputDeviceId) return;
  const selectable = audio as HTMLAudioElement & {
    setSinkId?: (deviceId: string) => Promise<void>;
  };
  if (typeof selectable.setSinkId !== "function") return;
  try {
    await selectable.setSinkId(outputDeviceId);
  } catch {
    throw failure("speaker_unavailable");
  }
}

export function waitForIceGathering(peer: RTCPeerConnection): Promise<void> {
  if (peer.iceGatheringState === "complete") return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      peer.removeEventListener("icegatheringstatechange", onChange);
      reject(failure("ice_timeout"));
    }, ICE_TIMEOUT_MS);
    const onChange = () => {
      if (peer.iceGatheringState !== "complete") return;
      window.clearTimeout(timeout);
      peer.removeEventListener("icegatheringstatechange", onChange);
      resolve();
    };
    peer.addEventListener("icegatheringstatechange", onChange);
  });
}

export function waitForDataChannelOpen(
  peer: RTCPeerConnection,
  channelState: () => RTCDataChannelState | "closed",
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    let poll: number | null = null;
    const cleanup = () => {
      if (poll !== null) window.clearInterval(poll);
      window.clearTimeout(timeout);
    };
    const timeout = window.setTimeout(() => {
      cleanup();
      reject(failure("peer_timeout"));
    }, PEER_READY_TIMEOUT_MS);
    poll = window.setInterval(() => {
      if (channelState() === "open") {
        cleanup();
        resolve();
        return;
      }
      if (peer.connectionState === "failed" || peer.connectionState === "closed") {
        cleanup();
        reject(failure("peer_failed"));
      }
    }, 50);
  });
}

export function createClientSessionId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function isUnavailableAudioDevice(error: unknown): boolean {
  const name = error instanceof DOMException ? error.name : "";
  return name === "NotFoundError" || name === "OverconstrainedError";
}
