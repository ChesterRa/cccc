export type WebModelActorSelection = { groupId: string; actorId: string };

export function resolveWebModelActorSelection(
  groupId: string,
  actorId: string,
): WebModelActorSelection | null {
  const normalizedGroupId = String(groupId || "").trim();
  const normalizedActorId = String(actorId || "").trim();
  if (!normalizedGroupId || !normalizedActorId) return null;
  return { groupId: normalizedGroupId, actorId: normalizedActorId };
}

export function matchesWebModelActorSelection(
  current: WebModelActorSelection,
  groupId: string,
  actorId: string,
): boolean {
  const candidate = resolveWebModelActorSelection(groupId, actorId);
  return (
    candidate !== null &&
    candidate.groupId === String(current.groupId || "").trim() &&
    candidate.actorId === String(current.actorId || "").trim()
  );
}
