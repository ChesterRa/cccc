export function resolveNewActorId(actorId: string, suggestedActorId: string): string {
  return String(actorId || "").trim() || String(suggestedActorId || "").trim();
}
