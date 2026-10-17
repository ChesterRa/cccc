import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import { useGroupStore } from "../stores/useGroupStore";

/** Subscribe to the inputs as well as the stable imperative getter. */
export function useOrderedGroups() {
  const { groups, groupOrder, selectedGroupId, groupDoc, actors, getOrderedGroups } = useGroupStore(
    useShallow((state) => ({
      groups: state.groups,
      groupOrder: state.groupOrder,
      selectedGroupId: state.selectedGroupId,
      groupDoc: state.groupDoc,
      actors: state.actors,
      getOrderedGroups: state.getOrderedGroups,
    })),
  );
  return useMemo(() => {
    void groups;
    void groupOrder;
    void selectedGroupId;
    void groupDoc;
    void actors;
    return getOrderedGroups();
  }, [actors, getOrderedGroups, groupDoc, groupOrder, groups, selectedGroupId]);
}
