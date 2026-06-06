import { describe, expect, it } from "vitest";

import type { Actor } from "../types";
import {
  computeGroupRuntimeFromActorActivityUpdates,
  getRuntimeStatusFallbackForGroup,
} from "./useSSE";

describe("SSE actor activity runtime projection", () => {
  it("does not derive stopped lifecycle from zero running actors", () => {
    const actors: Actor[] = [
      {
        id: "claude-1",
        role: "foreman",
        runtime: "codex",
        runner: "pty",
        running: true,
      },
    ];

    const runtime = computeGroupRuntimeFromActorActivityUpdates(
      actors,
      [{ id: "claude-1", running: false }],
      { lifecycle_state: "active" },
    );

    expect(runtime.lifecycle_state).toBe("active");
    expect(runtime.runtime_running).toBe(false);
    expect(runtime.running_actor_count).toBe(0);
  });

  it("preserves paused lifecycle while reporting running actors", () => {
    const runtime = computeGroupRuntimeFromActorActivityUpdates(
      [],
      [{ id: "claude-1", running: true }],
      { lifecycle_state: "paused" },
    );

    expect(runtime.lifecycle_state).toBe("paused");
    expect(runtime.runtime_running).toBe(true);
    expect(runtime.running_actor_count).toBe(1);
  });

  it("derives idle lifecycle from running actor activity updates", () => {
    const actors: Actor[] = [
      {
        id: "claude-1",
        role: "peer",
        runtime: "claude",
        runner: "pty",
        running: true,
        effective_working_state: "working",
      },
    ];

    const runtime = computeGroupRuntimeFromActorActivityUpdates(
      actors,
      [{ id: "claude-1", running: true, effective_working_state: "idle" }],
      { lifecycle_state: "active" },
    );

    expect(runtime.lifecycle_state).toBe("idle");
    expect(runtime.runtime_running).toBe(true);
    expect(runtime.running_actor_count).toBe(1);
  });

  it("uses the event group runtime fallback instead of the selected groupDoc", () => {
    const selectedRuntime = {
      lifecycle_state: "active",
      runtime_running: true,
      running_actor_count: 1,
      has_running_foreman: true,
    };
    const targetRuntime = {
      lifecycle_state: "stopped",
      runtime_running: false,
      running_actor_count: 0,
      has_running_foreman: false,
    };

    const fallback = getRuntimeStatusFallbackForGroup({
      groupDoc: {
        group_id: "selected",
        state: "active",
        running: true,
        runtime_status: selectedRuntime,
      },
      groups: [
        {
          group_id: "target",
          state: "stopped",
          running: false,
          runtime_status: targetRuntime,
        },
      ],
    } as ReturnType<typeof import("../stores").useGroupStore.getState>, "target");

    expect(fallback).toBe(targetRuntime);
    expect(computeGroupRuntimeFromActorActivityUpdates(
      [],
      [{ id: "claude-1", running: false }],
      fallback,
    ).lifecycle_state).toBe("stopped");
  });

  it("does not keep a stopped lifecycle once actors are running (wake from stopped)", () => {
    const runtime = computeGroupRuntimeFromActorActivityUpdates(
      [],
      [{ id: "claude-1", running: true, effective_working_state: "working" }],
      { lifecycle_state: "stopped", runtime_running: false, running_actor_count: 0, has_running_foreman: false },
    );

    // A running actor contradicts "stopped"; the projection must re-derive
    // instead of producing runtime_running:true + lifecycle_state:"stopped".
    expect(runtime.runtime_running).toBe(true);
    expect(runtime.lifecycle_state).toBe("active");
  });

  it("re-derives idle (not stopped) when a woken actor is not busy", () => {
    const runtime = computeGroupRuntimeFromActorActivityUpdates(
      [],
      [{ id: "claude-1", running: true, effective_working_state: "idle" }],
      { lifecycle_state: "stopped", runtime_running: false, running_actor_count: 0, has_running_foreman: false },
    );

    expect(runtime.runtime_running).toBe(true);
    expect(runtime.lifecycle_state).toBe("idle");
  });
});
