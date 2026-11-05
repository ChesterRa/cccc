import type { Actor } from "../types";

/** Match the daemon's Actor registration identity without guessing missing data. */
export function actorGenerationIdentity(actor: Actor): string | undefined {
  if (actor.generation) return actor.generation;
  return typeof actor.created_at === "string" ? `legacy:${actor.created_at}` : undefined;
}

/** An avatar write must not rebase an editor over other saved Actor changes. */
export function sameActorConfiguration(before: Actor, after: Actor): boolean {
  const generation = actorGenerationIdentity(before);
  return (
    generation !== undefined &&
    generation === actorGenerationIdentity(after) &&
    before.id === after.id &&
    (before.created_at || "") === (after.created_at || "") &&
    (before.role || "peer") === (after.role || "peer") &&
    (before.internal_kind || "") === (after.internal_kind || "") &&
    (before.title || "") === (after.title || "") &&
    (before.model_label || "") === (after.model_label || "") &&
    (before.external_session_id || "") === (after.external_session_id || "") &&
    (before.runtime || "") === (after.runtime || "") &&
    (before.runtime_mode || "default") === (after.runtime_mode || "default") &&
    (before.enabled ?? true) === (after.enabled ?? true) &&
    (before.default_scope_key || "") === (after.default_scope_key || "") &&
    (before.submit || "enter") === (after.submit || "enter") &&
    JSON.stringify(before.command || []) === JSON.stringify(after.command || []) &&
    JSON.stringify(Object.entries(before.env || {}).sort()) ===
      JSON.stringify(Object.entries(after.env || {}).sort()) &&
    JSON.stringify(before.capability_autoload || []) ===
      JSON.stringify(after.capability_autoload || []) &&
    JSON.stringify(before.capability_hidden || []) ===
      JSON.stringify(after.capability_hidden || []) &&
    (before.profile_id || "") === (after.profile_id || "") &&
    (before.profile_scope || "global") === (after.profile_scope || "global") &&
    (before.profile_owner || "") === (after.profile_owner || "") &&
    (before.profile_revision_applied || 0) === (after.profile_revision_applied || 0)
  );
}
