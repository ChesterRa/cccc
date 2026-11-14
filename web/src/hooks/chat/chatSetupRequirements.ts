import type { Actor, GroupDoc } from "../../types";

/** A group made entirely of remote sessions does not need a local project scope. */
export function chatSetupRequirements(
  groupId: string,
  projectRoot: string,
  actors: Actor[],
  running: boolean,
) {
  const remoteOnly =
    actors.length > 0 && actors.every((actor) => actor.external_session_id?.startsWith("remote:"));
  const needsScope = !!groupId && !projectRoot && !remoteOnly;
  const needsActors = !!groupId && actors.length === 0;
  const needsStart = !!groupId && actors.length > 0 && !running;
  return {
    needsScope,
    needsActors,
    needsStart,
    showSetupCard: needsScope || needsActors || needsStart,
  };
}

export function projectScopeRoot(groupDoc: GroupDoc | null): string {
  if (!groupDoc) return "";
  const key = String(groupDoc.active_scope_key || "");
  if (!key) return "";
  const scopes = Array.isArray(groupDoc.scopes) ? groupDoc.scopes : [];
  const hit = scopes.find((scope) => String(scope.scope_key || "") === key);
  return String(hit?.url || "");
}
