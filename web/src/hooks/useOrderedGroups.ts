import { useShallow } from "zustand/react/shallow";
import { useGroupStore } from "../stores/useGroupStore";

/** Subscribe to the inputs as well as the stable imperative getter. */
export function useOrderedGroups() {
  const { getOrderedGroups } = useGroupStore(
    useShallow((state) => ({
      groups: state.groups,
      groupOrder: state.groupOrder,
      selectedGroupId: state.selectedGroupId,
      groupDoc: state.groupDoc,
      actors: state.actors,
      getOrderedGroups: state.getOrderedGroups,
    })),
  );
  return getOrderedGroups();
}
