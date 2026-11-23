import { useGroupStore, useUIStore } from "../../stores";

/** Admin rights and the target Group are checked again here, because a pending-edit prompt can delay the call. */
export function canOpenExistingSessions(groupId: string) {
  const ui = useUIStore.getState();
  return (
    ui.isWebAdmin === true &&
    !ui.webReadOnly &&
    useGroupStore.getState().groups.some((group) => group.group_id === groupId)
  );
}
