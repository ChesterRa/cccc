export const CAPABILITY_CHANGED_EVENT = "cccc:capability-changed";
export const CAPABILITY_CHANGED_STORAGE_KEY = "cccc:capability-changed";

type CapabilityChangedPayload = {
  group_id: string;
  nonce?: string;
};

function normalizeGroupId(value: unknown): string {
  return String(value || "").trim();
}

function parsePayload(value: unknown): CapabilityChangedPayload | null {
  if (!value) return null;
  if (typeof value === "object") {
    const groupId = normalizeGroupId((value as { group_id?: unknown }).group_id);
    return groupId ? { group_id: groupId } : null;
  }
  try {
    const parsed = JSON.parse(String(value));
    const groupId = normalizeGroupId(parsed?.group_id);
    return groupId ? { group_id: groupId, nonce: normalizeGroupId(parsed?.nonce) } : null;
  } catch {
    return null;
  }
}

function matchesGroup(payload: CapabilityChangedPayload | null, groupId: string): boolean {
  const gid = normalizeGroupId(groupId);
  return Boolean(gid && payload?.group_id === gid);
}

export function publishCapabilityChanged(groupId: string): void {
  const gid = normalizeGroupId(groupId);
  if (!gid || typeof window === "undefined") return;
  const detail: CapabilityChangedPayload = { group_id: gid };
  window.dispatchEvent(new CustomEvent(CAPABILITY_CHANGED_EVENT, { detail }));
  try {
    window.localStorage.setItem(CAPABILITY_CHANGED_STORAGE_KEY, JSON.stringify({
      group_id: gid,
      nonce: `${Date.now()}:${Math.random()}`,
    }));
  } catch {
    void 0;
  }
}

export function subscribeCapabilityChanged(groupId: string, listener: () => void): () => void {
  const gid = normalizeGroupId(groupId);
  if (!gid || typeof window === "undefined") return () => {};

  const handleLocal = (event: Event) => {
    const detail = event instanceof CustomEvent ? event.detail : null;
    if (matchesGroup(parsePayload(detail), gid)) listener();
  };
  const handleStorage = (event: StorageEvent) => {
    if (event.storageArea !== window.localStorage || event.key !== CAPABILITY_CHANGED_STORAGE_KEY) return;
    if (matchesGroup(parsePayload(event.newValue), gid)) listener();
  };

  window.addEventListener(CAPABILITY_CHANGED_EVENT, handleLocal);
  window.addEventListener("storage", handleStorage);
  return () => {
    window.removeEventListener(CAPABILITY_CHANGED_EVENT, handleLocal);
    window.removeEventListener("storage", handleStorage);
  };
}
