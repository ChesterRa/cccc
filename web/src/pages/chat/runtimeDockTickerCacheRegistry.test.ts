import { describe, expect, it } from "vite-plus/test";

import {
  pruneRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
} from "./runtimeDockTickerCache";
import { createRuntimeDockTickerCacheRegistry } from "./runtimeDockTickerCacheRegistry";
import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

function messageEntry(id: string): RuntimeDockTickerEntry {
  return {
    id,
    kind: "message",
    actorId: "actor-1",
    actorLabel: "Actor 1",
    text: "Old completed response.",
    updatedAt: "2026-08-26T00:00:00.000Z",
    sourceId: id,
    completed: true,
  };
}

describe("runtime dock ticker group cache", () => {
  it("keeps retired entries retired after switching away and back", () => {
    const registry = createRuntimeDockTickerCacheRegistry();
    const entry = messageEntry("message-1");
    const firstCache = registry.get("group-a");

    expect(upsertRuntimeDockTickerCache(firstCache, [entry], 1_000)).toHaveLength(1);
    expect(pruneRuntimeDockTickerCache(firstCache, 7_000)).toEqual([]);

    registry.get("group-b");
    const restoredCache = registry.get("group-a");

    expect(restoredCache).toBe(firstCache);
    expect(upsertRuntimeDockTickerCache(restoredCache, [entry], 7_001)).toEqual([]);
  });

  it("does not share visible ticker state between groups", () => {
    const registry = createRuntimeDockTickerCacheRegistry();
    const groupA = registry.get("group-a");
    const groupB = registry.get("group-b");

    expect(groupB).not.toBe(groupA);
    expect(upsertRuntimeDockTickerCache(groupA, [messageEntry("message-1")], 1_000)).toHaveLength(
      1,
    );
    expect(pruneRuntimeDockTickerCache(groupB, 1_000)).toEqual([]);
  });
});
