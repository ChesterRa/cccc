type VoiceCaptureLock = {
  ownerId: string;
  groupId: string;
  updatedAt: number;
};

export type VoiceCaptureChannelMessage = {
  type?: "probe" | "alive";
  ownerId?: string;
  groupId?: string;
  sentAt?: number;
};

const VOICE_CAPTURE_LOCK_KEY = "cccc.voiceSecretary.activeCapture";
const VOICE_CAPTURE_CHANNEL_NAME = "cccc.voiceSecretary.capture";
const VOICE_CAPTURE_LOCK_TTL_MS = 30 * 1000;
const VOICE_CAPTURE_LOCK_PROBE_TIMEOUT_MS = 300;

export function createVoiceCaptureOwnerId(): string {
  return `voice-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function readVoiceCaptureLock(): VoiceCaptureLock | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(VOICE_CAPTURE_LOCK_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<VoiceCaptureLock>;
    const ownerId = String(parsed.ownerId || "").trim();
    const groupId = String(parsed.groupId || "").trim();
    const updatedAt = Number(parsed.updatedAt || 0);
    if (!ownerId || !groupId || !Number.isFinite(updatedAt)) return null;
    if (Date.now() - updatedAt > VOICE_CAPTURE_LOCK_TTL_MS) {
      window.localStorage.removeItem(VOICE_CAPTURE_LOCK_KEY);
      return null;
    }
    return { ownerId, groupId, updatedAt };
  } catch {
    return null;
  }
}

function writeVoiceCaptureLock(ownerId: string, groupId: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      VOICE_CAPTURE_LOCK_KEY,
      JSON.stringify({ ownerId, groupId, updatedAt: Date.now() }),
    );
  } catch {
    void 0;
  }
}

function clearVoiceCaptureLock(ownerId?: string): void {
  if (typeof window === "undefined") return;
  try {
    if (ownerId) {
      const active = readVoiceCaptureLock();
      if (active && active.ownerId !== ownerId) return;
    }
    window.localStorage.removeItem(VOICE_CAPTURE_LOCK_KEY);
  } catch {
    void 0;
  }
}

export function openVoiceCaptureChannel(): BroadcastChannel | null {
  if (typeof BroadcastChannel === "undefined") return null;
  try {
    return new BroadcastChannel(VOICE_CAPTURE_CHANNEL_NAME);
  } catch {
    return null;
  }
}

function probeVoiceCaptureOwner(lock: VoiceCaptureLock): Promise<boolean> {
  const channel = openVoiceCaptureChannel();
  if (!channel) return Promise.resolve(true);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (alive: boolean) => {
      if (settled) return;
      settled = true;
      channel.removeEventListener("message", handleMessage);
      channel.close();
      resolve(alive);
    };
    const handleMessage = (event: MessageEvent<VoiceCaptureChannelMessage>) => {
      const message = event.data || {};
      if (message.type !== "alive") return;
      if (String(message.ownerId || "") !== lock.ownerId) return;
      finish(true);
    };
    channel.addEventListener("message", handleMessage);
    window.setTimeout(() => finish(false), VOICE_CAPTURE_LOCK_PROBE_TIMEOUT_MS);
    channel.postMessage({
      type: "probe",
      ownerId: lock.ownerId,
      groupId: lock.groupId,
      sentAt: Date.now(),
    } satisfies VoiceCaptureChannelMessage);
  });
}

export async function claimVoiceCaptureLock(ownerId: string, groupId: string): Promise<VoiceCaptureLock | null> {
  const active = readVoiceCaptureLock();
  if (active && active.ownerId !== ownerId) {
    const ownerAlive = await probeVoiceCaptureOwner(active);
    if (ownerAlive) return active;
    clearVoiceCaptureLock(active.ownerId);
  }
  writeVoiceCaptureLock(ownerId, groupId);
  return null;
}

export function refreshVoiceCaptureLock(ownerId: string, groupId: string): void {
  const active = readVoiceCaptureLock();
  if (!active || active.ownerId === ownerId) writeVoiceCaptureLock(ownerId, groupId);
}

export function releaseVoiceCaptureLock(ownerId: string): void {
  clearVoiceCaptureLock(ownerId);
}
