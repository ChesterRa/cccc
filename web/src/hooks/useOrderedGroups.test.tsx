// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { useGroupStore } from "../stores/useGroupStore";
import type { GroupMeta } from "../types";
import { useOrderedGroups } from "./useOrderedGroups";

function OrderedGroupList() {
  return (
    <output>
      {useOrderedGroups()
        .map((group) => group.group_id)
        .join(",")}
    </output>
  );
}

describe("useOrderedGroups", () => {
  const initialState = useGroupStore.getState();
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    localStorage.clear();
    useGroupStore.setState({
      groups: ["a", "b", "c", "d"].map((group_id) => ({ group_id }) as GroupMeta),
      groupOrder: ["a", "b", "c", "d"],
      archivedGroupIds: ["b", "d"],
      selectedGroupId: "",
      groupDoc: null,
      actors: [],
    });
    host = document.createElement("div");
    root = createRoot(host);
    await act(async () => root.render(<OrderedGroupList />));
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    useGroupStore.setState(initialState, true);
    localStorage.clear();
  });

  it.each(["working", "archived"] as const)(
    "immediately renders and persists consecutive %s reorders without a parent render",
    async (section) => {
      const groups = useGroupStore.getState().groups;
      await act(async () => useGroupStore.getState().reorderGroupsInSection(section, 0, 1));
      const expected = section === "working" ? ["c", "b", "a", "d"] : ["a", "d", "c", "b"];
      expect(host.textContent).toBe(expected.join(","));
      expect(JSON.parse(localStorage.getItem("cccc-group-order")!)).toEqual(expected);
      expect(useGroupStore.getState().groups).toBe(groups);

      await act(async () => useGroupStore.getState().reorderGroupsInSection(section, 1, 0));
      expect(host.textContent).toBe("a,b,c,d");
    },
  );
});
