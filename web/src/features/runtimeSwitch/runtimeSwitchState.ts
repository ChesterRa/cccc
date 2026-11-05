import type { Actor, GroupDoc } from "../../types";
import { actorGenerationIdentity } from "../../utils/actorIdentity";
import * as api from "../../services/api";
import { useGroupStore, useModalStore } from "../../stores";

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

/** Open only the member that completed the switch, without replacing another editor. */
export function openWebSetup(
  groupId: string,
  actorId: string,
  runtime: string,
  generation: string | undefined,
): "opened" | "editor_open" | "unavailable" {
  const group = useGroupStore.getState();
  const actor = group.actors.find(
    (item) =>
      item.id === actorId &&
      item.runtime === runtime &&
      actorGenerationIdentity(item) === generation,
  );
  if (group.selectedGroupId !== groupId || !actor) return "unavailable";
  if (useModalStore.getState().editingActor) return "editor_open";
  useModalStore.getState().openActorEditor(actor, "chatgpt");
  return "opened";
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
