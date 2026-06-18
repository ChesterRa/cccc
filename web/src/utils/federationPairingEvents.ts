export const FEDERATION_PAIRING_CHANGED_EVENT = "cccc:federation-pairing-changed";

type FederationPairingChangedPayload = {
  group_id: string;
};

function normalizeGroupId(value: unknown): string {
  return String(value || "").trim();
}

function parsePayload(value: unknown): FederationPairingChangedPayload | null {
  if (!value || typeof value !== "object") return null;
  const groupId = normalizeGroupId((value as { group_id?: unknown }).group_id);
  return groupId ? { group_id: groupId } : null;
}

export function publishFederationPairingChanged(groupId: string): void {
  const gid = normalizeGroupId(groupId);
  if (!gid || typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(FEDERATION_PAIRING_CHANGED_EVENT, {
    detail: { group_id: gid },
  }));
}

export function subscribeFederationPairingChanged(groupId: string, listener: () => void): () => void {
  const gid = normalizeGroupId(groupId);
  if (!gid || typeof window === "undefined") return () => {};

  const handleLocal = (event: Event) => {
    const detail = event instanceof CustomEvent ? event.detail : null;
    const payload = parsePayload(detail);
    if (payload?.group_id === gid) listener();
  };

  window.addEventListener(FEDERATION_PAIRING_CHANGED_EVENT, handleLocal);
  return () => window.removeEventListener(FEDERATION_PAIRING_CHANGED_EVENT, handleLocal);
}
