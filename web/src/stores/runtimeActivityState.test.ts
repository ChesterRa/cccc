import { describe, expect, it } from "vite-plus/test";
import type { RuntimeActivityEvent } from "../types";
import {
  ingestRuntimeActivityEvents,
  mergeRuntimeActivityEvents,
  pruneRuntimeActivityEvents,
} from "./runtimeActivityState";
import { useRuntimeActivityStore } from "./useRuntimeActivityStore";

function event(
  id: string,
  status: RuntimeActivityEvent["status"],
  ts: string,
): RuntimeActivityEvent {
  return {
    v: 1,
    id,
    ts,
    group_id: "g1",
    actor_id: "peer",
    runtime: "codex",
    activity_id: "tool:1",
    kind: "tool",
    status,
    event_type: "PreToolUse",
    session_id: "session",
  };
}

describe("runtime activity state", () => {
  it("keeps only the latest revision for one activity", () => {
    const merged = mergeRuntimeActivityEvents(
      [event("started", "started", "2026-07-28T00:00:00Z")],
      [event("completed", "completed", "2026-07-28T00:00:02Z")],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.status).toBe("completed");
  });

  it("lets a real terminal revision replace a newer synthetic stuck revision", () => {
    const merged = mergeRuntimeActivityEvents(
      [event("stuck", "stuck", "2026-07-28T00:01:00Z")],
      [event("completed", "completed", "2026-07-28T00:00:59Z")],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.status).toBe("completed");
  });

  it("does not let a later synthetic stuck revision replace a real terminal revision", () => {
    const merged = mergeRuntimeActivityEvents(
      [event("failed", "failed", "2026-07-28T00:00:59Z")],
      [event("stuck", "stuck", "2026-07-28T00:01:00Z")],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.status).toBe("failed");
  });

  it("accepts a later real start when a provider reuses an activity identity", () => {
    const merged = mergeRuntimeActivityEvents(
      [event("completed", "completed", "2026-07-28T00:00:59Z")],
      [event("restarted", "started", "2026-07-28T00:01:00Z")],
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.status).toBe("started");
  });

  it("rejects cross-group events", () => {
    const foreign = { ...event("event", "started", "2026-07-28T00:00:00Z"), group_id: "g2" };
    expect(ingestRuntimeActivityEvents({}, "g1", [foreign])).toEqual({});
  });

  it("clears a group's activities when its stream lifecycle ends", () => {
    const store = useRuntimeActivityStore.getState();
    store.ingest("g1", [event("started", "started", "2026-07-28T00:00:00Z")]);
    expect(useRuntimeActivityStore.getState().byGroup.g1?.peer).toHaveLength(1);

    store.clearGroup("g1");

    expect(useRuntimeActivityStore.getState().byGroup.g1).toBeUndefined();
  });

  it("expires completed events before active and stuck events", () => {
    const state = ingestRuntimeActivityEvents({}, "g1", [
      event("completed", "completed", "2026-07-28T00:00:00Z"),
      { ...event("stuck", "stuck", "2026-07-28T00:00:01Z"), activity_id: "turn:1" },
    ]);
    const pruned = pruneRuntimeActivityEvents(state, Date.parse("2026-07-28T00:00:10Z"));
    expect(pruned.g1?.peer).toHaveLength(1);
    expect(pruned.g1?.peer?.[0]?.status).toBe("stuck");
  });
});
