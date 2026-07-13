import { useMemo } from "react";

import type { Actor, GroupDoc, GroupMeta, GroupRuntimeStatus } from "../types";
import { getGroupRuntimeStatus } from "../utils/groupStatus";
import { computeGroupRuntimePatch } from "../utils/groupRuntimeProjection";

type UseSelectedGroupRuntimeArgs = {
  groups: GroupMeta[];
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  actors: Actor[];
};

type SelectedGroupRuntime = {
  selectedGroupMeta: GroupMeta | null;
  selectedGroupRunning: boolean;
  selectedGroupRuntimeStatus: GroupRuntimeStatus;
  orderedSelectedGroupPatch: Pick<GroupMeta, "running" | "state" | "runtime_status"> | null;
};

export function computeSelectedGroupRuntime({
  groups,
  selectedGroupId,
  groupDoc,
  actors,
}: UseSelectedGroupRuntimeArgs): SelectedGroupRuntime {
  const selectedGroupMeta =
    groups.find((group) => String(group.group_id || "") === selectedGroupId) || null;
  const orderedSelectedGroupPatch = selectedGroupMeta
    ? computeGroupRuntimePatch({ group: selectedGroupMeta, groupDoc, actors })
    : null;
  const selectedGroupRuntimeStatus =
    orderedSelectedGroupPatch?.runtime_status || getGroupRuntimeStatus(groupDoc);

  return {
    selectedGroupMeta,
    selectedGroupRunning: selectedGroupRuntimeStatus.runtime_running,
    selectedGroupRuntimeStatus,
    orderedSelectedGroupPatch: selectedGroupId ? orderedSelectedGroupPatch : null,
  };
}

export function useSelectedGroupRuntime(args: UseSelectedGroupRuntimeArgs): SelectedGroupRuntime {
  return useMemo(() => computeSelectedGroupRuntime(args), [args]);
}
