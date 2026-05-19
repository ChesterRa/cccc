import type { Actor } from "../../types";

export function resolveRuntimeInspectorActor(
  actorId: string,
  runtimeActors: Actor[],
  mountedActorsById: Record<string, Actor>,
): Actor | null {
  const id = String(actorId || "").trim();
  if (!id) return null;
  return runtimeActors.find((item) => String(item.id || "").trim() === id) || mountedActorsById[id] || null;
}
