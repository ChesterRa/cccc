import { beforeEach, describe, expect, it } from "vite-plus/test";

import type { Actor } from "../types";
import { useComposerStore } from "./useComposerStore";
import { useGroupStore } from "./useGroupStore";

describe("group selection", () => {
  const actors: Actor[] = [
    { id: "implementer", role: "peer", runtime: "codex", runner: "headless", running: true },
  ];

  beforeEach(() => {
    useGroupStore.setState({
      selectedGroupId: "g_active",
      actors,
      selectedGroupActorsHydrating: false,
      selectedGroupActorStatusProvisional: false,
    });
    useComposerStore.setState({ activeGroupId: "g_active", destGroupId: "g_active" });
  });

  it("does not re-enter actor hydration when the active group is selected again", () => {
    useGroupStore.getState().setSelectedGroupId("g_active");

    const state = useGroupStore.getState();
    expect(state.selectedGroupId).toBe("g_active");
    expect(state.actors).toBe(actors);
    expect(state.selectedGroupActorsHydrating).toBe(false);
    expect(state.selectedGroupActorStatusProvisional).toBe(false);
  });

  it("still repairs composer ownership during an idempotent group selection", () => {
    useComposerStore.setState({ activeGroupId: "g_stale", destGroupId: "g_stale" });

    useGroupStore.getState().setSelectedGroupId("g_active");

    expect(useComposerStore.getState().activeGroupId).toBe("g_active");
    expect(useComposerStore.getState().destGroupId).toBe("g_active");
    expect(useGroupStore.getState().selectedGroupActorsHydrating).toBe(false);
    expect(useGroupStore.getState().selectedGroupActorStatusProvisional).toBe(false);
  });

  it("ignores actor activity delivered by the previous group after selection changes", () => {
    useGroupStore
      .getState()
      .updateActorActivity(
        [{ id: "implementer", running: false, effective_working_state: "stopped" }],
        "g_previous",
      );

    expect(useGroupStore.getState().actors).toBe(actors);
    expect(useGroupStore.getState().actors[0].running).toBe(true);
  });
});
