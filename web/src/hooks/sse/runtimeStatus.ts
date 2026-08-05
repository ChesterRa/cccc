import type { Actor, GroupRuntimeStatus } from "../../types";
import type { useGroupStore } from "../../stores";

export type ActorActivityUpdate = {
  id: string;
  idle_seconds?: number | null;
  running: boolean;
  effective_working_state?: string;
  effective_working_reason?: string;
  effective_working_updated_at?: string | null;
  effective_active_task_id?: string | null;
  runtime_session_status?: string | null;
  runtime_session_resume_eligible?: boolean | null;
  runtime_session_last_resume_error?: string | null;
};

export function getRuntimeStatusFallbackForGroup(
  state: ReturnType<typeof useGroupStore.getState>,
  groupId: string,
): GroupRuntimeStatus | null {
  const gid = String(groupId || "").trim();
  if (!gid) return null;
  if (String(state.groupDoc?.group_id || "").trim() === gid) {
    return state.groupDoc?.runtime_status || null;
  }
  return (
    state.groups.find((group) => String(group.group_id || "").trim() === gid)?.runtime_status ||
    null
  );
}

export function computeGroupRuntimeFromActorActivityUpdate(
  actors: Actor[],
  update: ActorActivityUpdate,
  fallback?: GroupRuntimeStatus | null,
): GroupRuntimeStatus {
  return computeGroupRuntimeFromActorActivityUpdates(actors, [update], fallback);
}

function deriveLifecycleState(
  runningActors: Actor[],
  fallback?: GroupRuntimeStatus | null,
): string {
  const fallbackLifecycle = String(fallback?.lifecycle_state || "active");
  if (runningActors.length === 0 || fallbackLifecycle === "paused") return fallbackLifecycle;
  return runningActors.some((actor) => {
    const state = String(actor.effective_working_state || "")
      .trim()
      .toLowerCase();
    return state === "working" || state === "waiting" || state === "stuck";
  })
    ? "active"
    : "idle";
}

export function computeGroupRuntimeFromActorActivityUpdates(
  actors: Actor[],
  updates: ActorActivityUpdate[],
  fallback?: GroupRuntimeStatus | null,
): GroupRuntimeStatus {
  const actorById = new Map<string, Actor>();
  for (const actor of actors || []) {
    const actorId = String(actor?.id || "").trim();
    if (actorId) actorById.set(actorId, actor);
  }
  for (const update of updates || []) {
    const actorId = String(update.id || "").trim();
    if (!actorId) continue;
    actorById.set(actorId, {
      ...(actorById.get(actorId) || { id: actorId }),
      ...update,
      id: actorId,
    });
  }

  const runningActors = Array.from(actorById.values()).filter((actor) => !!actor.running);
  return {
    lifecycle_state: deriveLifecycleState(runningActors, fallback),
    runtime_running: runningActors.length > 0,
    running_actor_count: runningActors.length,
    has_running_foreman: runningActors.some(
      (actor) =>
        String(actor.role || "")
          .trim()
          .toLowerCase() === "foreman",
    ),
  };
}
