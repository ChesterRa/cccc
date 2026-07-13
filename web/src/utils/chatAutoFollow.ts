export interface ChatTailSnapshot {
  count: number;
  tailKey: string | number | null;
}

export interface ChatTailMutationSnapshot {
  tailKey: string | number | null;
  signature: string;
}

export function getChatTailSnapshot(
  tailKey: string | number | null,
  count: number,
): ChatTailSnapshot {
  return { count: Math.max(0, Number(count) || 0), tailKey };
}

export function shouldAutoFollowOnTailAppend(
  prev: ChatTailSnapshot,
  next: ChatTailSnapshot,
): boolean {
  if (prev.count <= 0 || next.count <= prev.count) return false;
  if (prev.tailKey == null || next.tailKey == null) return false;
  return prev.tailKey !== next.tailKey;
}

export function getChatTailMutationSnapshot(
  tailKey: string | number | null,
  signature: string,
): ChatTailMutationSnapshot {
  return { tailKey, signature: String(signature || "") };
}

export function shouldAutoFollowOnTailMutation(
  prev: ChatTailMutationSnapshot,
  next: ChatTailMutationSnapshot,
): boolean {
  if (prev.tailKey == null || next.tailKey == null) return false;
  if (prev.tailKey !== next.tailKey) return false;
  if (!prev.signature || !next.signature) return false;
  return prev.signature !== next.signature;
}
