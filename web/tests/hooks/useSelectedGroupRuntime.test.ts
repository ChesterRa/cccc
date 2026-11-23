import { describe, expect, it } from "vite-plus/test";

import { chatSetupRequirements } from "../../src/hooks/chat/chatSetupRequirements";
import { computeSelectedGroupRuntime } from "../../src/hooks/useSelectedGroupRuntime";
import { computeGroupRuntimePatch } from "../../src/utils/groupRuntimeProjection";

describe("useSelectedGroupRuntime", () => {
  it("prefers live actor runtime over stale group meta", () => {
    const result = computeSelectedGroupRuntime({
      groups: [{ group_id: "g1", running: false, state: "active" }],
      selectedGroupId: "g1",
      groupDoc: { group_id: "g1", state: "active" },
      actors: [{ id: "a1", running: true }],
    });

    expect(result.selectedGroupRunning).toBe(true);
    expect(result.selectedGroupRuntimeStatus.runtime_running).toBe(true);
  });

  it("ignores stale groupDoc runtime_status when live actors are running", () => {
    const result = computeSelectedGroupRuntime({
      groups: [{ group_id: "g1", running: false, state: "active" }],
      selectedGroupId: "g1",
      groupDoc: {
        group_id: "g1",
        state: "active",
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      },
      actors: [{ id: "a1", running: true }],
    });

    expect(result.selectedGroupRunning).toBe(true);
    expect(result.selectedGroupRuntimeStatus).toEqual({
      lifecycle_state: "active",
      runtime_running: true,
      running_actor_count: 0,
      has_running_foreman: false,
    });
  });

  it("keeps an unobserved remote-only Group unknown without prompting to start or attach", () => {
    const actors = [
      {
        id: "a1",
        running: true,
        external_session_id: "remote:fixture",
        external_session_kind: "device" as const,
      },
    ];
    const result = computeSelectedGroupRuntime({
      groups: [
        {
          group_id: "g1",
          state: "active",
          runtime_status: { lifecycle_state: "active", available: false },
        },
      ],
      selectedGroupId: "g1",
      groupDoc: {
        group_id: "g1",
        running: true,
        state: "active",
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: true,
          running_actor_count: 2,
          has_running_foreman: true,
        },
      },
      actors,
    });

    expect(result.selectedGroupRunning).toBeNull();
    expect(result.selectedGroupRuntimeStatus).toEqual({
      lifecycle_state: "active",
      available: false,
    });
    expect(result.orderedSelectedGroupPatch?.running).toBeUndefined();
    expect(chatSetupRequirements("g1", "", actors, result.selectedGroupRunning).showSetupCard).toBe(
      false,
    );
  });

  it("produces a sidebar patch for the selected group", () => {
    const result = computeSelectedGroupRuntime({
      groups: [{ group_id: "g1", running: false, state: "paused" }],
      selectedGroupId: "g1",
      groupDoc: {
        group_id: "g1",
        runtime_status: {
          lifecycle_state: "paused",
          runtime_running: true,
          running_actor_count: 1,
          has_running_foreman: true,
        },
      },
      actors: [],
    });

    expect(result.orderedSelectedGroupPatch).toEqual({
      running: true,
      state: "paused",
      runtime_status: {
        lifecycle_state: "paused",
        runtime_running: true,
        running_actor_count: 1,
        has_running_foreman: true,
      },
    });
  });

  it("prefers cached actor runtime over stale group meta for non-selected groups", () => {
    const patch = computeGroupRuntimePatch({
      group: { group_id: "g2", running: false, state: "active" },
      groupDoc: {
        group_id: "g2",
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      },
      actors: [{ id: "a2", running: true }],
    });

    expect(patch).toEqual({
      running: true,
      state: "active",
      runtime_status: {
        lifecycle_state: "active",
        runtime_running: true,
        running_actor_count: 0,
        has_running_foreman: false,
      },
    });
  });

  it("does not let a missing groupDoc default overwrite group meta runtime state", () => {
    const patch = computeGroupRuntimePatch({
      group: {
        group_id: "g2",
        running: false,
        state: "idle",
        runtime_status: {
          lifecycle_state: "idle",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      },
      groupDoc: null,
      actors: [],
    });

    expect(patch).toEqual({
      running: false,
      state: "idle",
      runtime_status: {
        lifecycle_state: "idle",
        runtime_running: false,
        running_actor_count: 0,
        has_running_foreman: false,
      },
    });
  });
});
