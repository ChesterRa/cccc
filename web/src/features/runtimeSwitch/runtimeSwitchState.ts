import type { Actor, GroupDoc } from "../../types";
import { actorGenerationIdentity } from "../../utils/actorIdentity";
import * as api from "../../services/api";
import { useGroupStore } from "../../stores";

export function hasPendingActorSwitch(
  group: GroupDoc | null,
  actor: Actor,
  groupId: string,
): boolean {
  if (group?.group_id !== groupId || !Array.isArray(group.runtime_switches)) return false;
  const generation = actorGenerationIdentity(actor);
  return group.runtime_switches.some(
    (entry) =>
      entry &&
      typeof entry === "object" &&
      entry.actor_id === actor.id &&
      entry.pending === true &&
      (!entry.actor_generation || !generation || entry.actor_generation === generation),
  );
}

export async function refreshRuntimeSwitchGroup(groupId: string): Promise<void> {
  const store = useGroupStore.getState();
  const [group] = await Promise.all([
    api.fetchGroup(groupId, { noCache: true }),
    store.refreshActors(groupId),
    store.refreshGroups(),
  ]);
  if (!group.ok) throw new Error(group.error.message);
  if (useGroupStore.getState().selectedGroupId === groupId) {
    useGroupStore.getState().setGroupDoc(group.result.group);
  }
}
