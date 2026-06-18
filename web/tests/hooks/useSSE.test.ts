import { describe, expect, it, vi } from "vitest";

const { localStorageMock } = vi.hoisted(() => {
  function makeStorage() {
    const data = new Map<string, string>();
    return {
      getItem: vi.fn((key: string) => data.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => {
        data.set(key, String(value));
      }),
      removeItem: vi.fn((key: string) => {
        data.delete(key);
      }),
      clear: vi.fn(() => {
        data.clear();
      }),
    };
  }

  const storage = makeStorage();
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("window", { setTimeout, clearTimeout, localStorage: storage });
  return { localStorageMock: storage };
});

import {
  GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS,
  computeGroupRuntimeFromActorActivityUpdate,
  computeGroupRuntimeFromActorActivityUpdates,
  getGroupStreamsHiddenDisconnectDelayMs,
  shouldStartGroupStreams,
} from "../../src/hooks/useSSE";
import type { Actor } from "../../src/types";

void localStorageMock;

describe("computeGroupRuntimeFromActorActivityUpdate", () => {
  it("starts group SSE only while visible but defers hidden disconnect with a grace period", () => {
    expect(shouldStartGroupStreams(false)).toBe(true);
    expect(shouldStartGroupStreams(true)).toBe(false);
    expect(getGroupStreamsHiddenDisconnectDelayMs(false)).toBeNull();
    expect(getGroupStreamsHiddenDisconnectDelayMs(true)).toBe(GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS);
    expect(GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS).toBeGreaterThanOrEqual(60_000);
  });

  it("keeps the group active while another actor is still working", () => {
    const actors: Actor[] = [
      { id: "peer-1", running: true, effective_working_state: "working" },
      { id: "peer-2", running: true, effective_working_state: "working" },
    ];

    expect(computeGroupRuntimeFromActorActivityUpdate(actors, {
      id: "peer-1",
      running: true,
      effective_working_state: "idle",
    })).toMatchObject({
      lifecycle_state: "active",
      runtime_running: true,
      running_actor_count: 2,
    });
  });

  it("marks the group idle when the completed actor was the only busy actor", () => {
    const actors: Actor[] = [
      { id: "peer-1", running: true, effective_working_state: "working" },
      { id: "peer-2", running: true, effective_working_state: "idle" },
    ];

    expect(computeGroupRuntimeFromActorActivityUpdate(actors, {
      id: "peer-1",
      running: true,
      effective_working_state: "idle",
    })).toMatchObject({
      lifecycle_state: "idle",
      runtime_running: true,
      running_actor_count: 2,
    });
  });

  it("preserves stopped fallback when the last running actor stops", () => {
    const actors: Actor[] = [
      { id: "peer-1", running: true, effective_working_state: "working" },
    ];

    expect(computeGroupRuntimeFromActorActivityUpdate(actors, {
      id: "peer-1",
      running: false,
      effective_working_state: "stopped",
    }, {
      lifecycle_state: "stopped",
      runtime_running: false,
      running_actor_count: 0,
      has_running_foreman: false,
    })).toMatchObject({
      lifecycle_state: "stopped",
      runtime_running: false,
      running_actor_count: 0,
    });
  });

  it("derives full runtime fields from batched actor.activity updates", () => {
    const actors: Actor[] = [
      { id: "foreman", role: "foreman", running: true, effective_working_state: "working" },
      { id: "peer-1", role: "peer", running: true, effective_working_state: "idle" },
    ];

    expect(computeGroupRuntimeFromActorActivityUpdates(actors, [
      { id: "foreman", running: false, effective_working_state: "stopped" },
      { id: "peer-1", running: true, effective_working_state: "idle" },
    ])).toMatchObject({
      lifecycle_state: "idle",
      runtime_running: true,
      running_actor_count: 1,
      has_running_foreman: false,
    });
  });
});
