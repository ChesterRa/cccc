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

  it("clears a running observation the daemon could no longer make, and restores it later", () => {
    useGroupStore.setState({
      actors: [
        {
          ...actors[0],
          runtime_state_source: "managed_session",
          runner_effective: "headless",
          effective_runtime: "codex",
          runtime_session_status: "bound",
          runtime_confirmation_required: false,
        },
      ],
    });
    const store = useGroupStore.getState();
    store.updateActorActivity(
      [
        {
          id: "implementer",
          effective_working_state: "unknown",
          effective_working_reason: "runtime_owner_unavailable",
          runtime_observation_error: "session owner could not confirm this member",
        },
      ],
      "g_active",
    );
    let actor = useGroupStore.getState().actors[0];
    // Everything the daemon leaves out of an agent it cannot observe goes, not only running.
    for (const key of [
      "running",
      "runtime_state_source",
      "runner_effective",
      "effective_runtime",
      "runtime_session_status",
      "runtime_confirmation_required",
    ]) {
      expect(actor).not.toHaveProperty(key);
    }
    expect(actor.runtime_observation_error).toBe("session owner could not confirm this member");

    store.updateActorActivity(
      [{ id: "implementer", running: true, effective_working_state: "idle" }],
      "g_active",
    );
    actor = useGroupStore.getState().actors[0];
    expect(actor.running).toBe(true);
    expect(actor.runtime_observation_error).toBeUndefined();
  });

  it("replaces the Group's running observation when the daemon reports it unobservable", () => {
    const observed = {
      lifecycle_state: "active",
      runtime_running: true,
      running_actor_count: 2,
      has_running_foreman: true,
    };
    useGroupStore.setState({
      groups: [{ group_id: "g_active", running: true, state: "active", runtime_status: observed }],
      groupDoc: { group_id: "g_active", running: true, state: "active", runtime_status: observed },
    });

    useGroupStore
      .getState()
      .updateGroupRuntimeState("g_active", { lifecycle_state: "active", available: false });

    const { groups, groupDoc } = useGroupStore.getState();
    const unobservable = { lifecycle_state: "active", available: false };
    expect(groups[0].runtime_status).toEqual(unobservable);
    expect(groups[0].running).toBeUndefined();
    expect(groupDoc?.runtime_status).toEqual(unobservable);
    expect(groupDoc?.running).toBeUndefined();
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
