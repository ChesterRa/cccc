import type { Actor, GroupDoc, GroupMeta } from "../types";
import { getGroupRuntimeStatus } from "./groupStatus";

type GroupRuntimePatchArgs = {
  group: Pick<GroupMeta, "running" | "state" | "runtime_status">;
  groupDoc?: Pick<GroupDoc, "running" | "state" | "runtime_status"> | null;
  actors?: Actor[];
};

export function computeGroupRuntimePatch({
  group,
  groupDoc,
  actors,
}: GroupRuntimePatchArgs): Pick<GroupMeta, "running" | "state" | "runtime_status"> {
  const metaRuntime = getGroupRuntimeStatus(group);
  const hasDocRuntime = Boolean(
    groupDoc &&
    (groupDoc.runtime_status ||
      typeof groupDoc.state === "string" ||
      typeof groupDoc.running === "boolean"),
  );
  const docRuntime = hasDocRuntime ? getGroupRuntimeStatus(groupDoc) : null;
  const runtimeRunning =
    Boolean(docRuntime?.runtime_running) ||
    metaRuntime.runtime_running ||
    (Array.isArray(actors) && actors.some((actor) => !!actor.running));

  const runtimeStatus = { ...metaRuntime, ...(docRuntime || {}), runtime_running: runtimeRunning };

  return {
    running: runtimeStatus.runtime_running,
    state: runtimeStatus.lifecycle_state as GroupMeta["state"],
    runtime_status: runtimeStatus,
  };
}
