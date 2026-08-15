export type VoiceRequestDispatchGate = {
  tryAcquire: (key: string) => boolean;
  release: (key: string) => void;
  isActive: (key: string) => boolean;
};

export function createVoiceRequestDispatchGate(): VoiceRequestDispatchGate {
  const active = new Set<string>();
  return {
    tryAcquire(key) {
      const normalized = String(key || "").trim();
      if (!normalized || active.has(normalized)) return false;
      active.add(normalized);
      return true;
    },
    release(key) {
      active.delete(String(key || "").trim());
    },
    isActive(key) {
      return active.has(String(key || "").trim());
    },
  };
}
