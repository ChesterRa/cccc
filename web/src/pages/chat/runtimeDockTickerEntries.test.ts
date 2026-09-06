import { describe, expect, it } from "vite-plus/test";
import type { HeadlessStreamEvent } from "../../types";
import { buildRuntimeDockTickerEntries } from "./runtimeDockTickerEntries";
import type { RuntimeDockItem } from "./runtimeDockItems";
import {
  createRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
  pruneRuntimeDockTickerCache,
  hasRuntimeDockTickerWork,
} from "./runtimeDockTickerCache";

function event(id: string, text: string, receivedAt?: number): HeadlessStreamEvent {
  return {
    id,
    actor_id: "a",
    type: "headless.message.completed",
    ts: "2026-09-04T00:00:00Z",
    data: { stream_id: "turn-1", text },
    _receivedAt: receivedAt,
  };
}
const item = { actorId: "a", actorLabel: "Manager", liveWorkCard: null } as RuntimeDockItem;
function entries(events: HeadlessStreamEvent[]) {
  return buildRuntimeDockTickerEntries([item], { a: events });
}

describe("live runtime progress bubbles", () => {
  it.each(["claude", "codex", "grok", "opencode", "kilo"])(
    "never turns %s restored history into new bubbles",
    (runtime) => {
      const actorItem = { ...item, runtime };
      expect(
        buildRuntimeDockTickerEntries([actorItem], { a: [event("old", "Old completed results")] }),
      ).toEqual([]);
    },
  );
  it("keeps a single current excerpt instead of replaying a long completed transcript", () => {
    const cache = createRuntimeDockTickerCache();
    const text = Array.from({ length: 29 }, (_, i) => `Progress paragraph ${i}.`).join("\n");
    const visible = upsertRuntimeDockTickerCache(cache, entries([event("new", text, 1000)]), 1000);
    expect(visible.map((e) => e.text)).toEqual(["Progress paragraph 28."]);
    expect(pruneRuntimeDockTickerCache(cache, 7100)).toEqual([]);
    expect(hasRuntimeDockTickerWork(cache)).toBe(false);
  });
  it("cannot revive expired progress after a fresh cache or a group eviction", () => {
    const data = entries([event("old", "Already shown", 1000)]);
    expect(upsertRuntimeDockTickerCache(createRuntimeDockTickerCache(), data, 9000)).toEqual([]);
  });
  it("coalesces a completion echo without extending visibility", () => {
    const cache = createRuntimeDockTickerCache();
    upsertRuntimeDockTickerCache(cache, entries([event("delta", "Complete result", 1000)]), 1000);
    upsertRuntimeDockTickerCache(cache, entries([event("done", "Complete result", 6500)]), 6500);
    expect(pruneRuntimeDockTickerCache(cache, 7001)).toEqual([]);
  });
  it("permits identical prose from a new turn and replaces previous prose in place", () => {
    const cache = createRuntimeDockTickerCache();
    upsertRuntimeDockTickerCache(cache, entries([event("one", "Passed", 1000)]), 1000);
    const next = event("two", "Passed", 9000);
    next.data!.stream_id = "turn-2";
    expect(upsertRuntimeDockTickerCache(cache, entries([next]), 9000)).toHaveLength(1);
    expect(
      upsertRuntimeDockTickerCache(
        cache,
        entries([event("three", "Changed result", 9100)]),
        9100,
      ).map((e) => e.text),
    ).toEqual(["Changed result"]);
  });
  it("shows at most two actors, with at most one bubble for each", () => {
    const cache = createRuntimeDockTickerCache();
    const data = entries([event("new", "Progress", 1000)])[0]!;
    const visible = upsertRuntimeDockTickerCache(
      cache,
      [
        data,
        { ...data, actorId: "b", id: "b", receivedAt: 1100 },
        { ...data, actorId: "c", id: "c", receivedAt: 1200 },
      ],
      1300,
    );
    expect(visible.map((e) => e.actorId)).toEqual(["b", "c"]);
  });
  it("preserves complete Unicode characters in a bounded excerpt", () => {
    const cache = createRuntimeDockTickerCache();
    const visible = upsertRuntimeDockTickerCache(
      cache,
      entries([event("new", "📌".repeat(200), 1000)]),
      1000,
    );
    expect(Array.from(visible[0]!.text)).toHaveLength(120);
    expect(visible[0]!.text).toBe("…" + "📌".repeat(119));
  });
  it("waits for a matching delta projection rather than showing old text as fresh", () => {
    const live = event("delta", "", 1000);
    live.type = "headless.message.delta";
    live.data = { stream_id: "turn-1", delta: " new" };
    expect(entries([live])).toEqual([]);
    const projected = {
      ...item,
      liveWorkCard: {
        transcriptBlocks: [
          { streamId: "turn-1", streamPhase: "", updatedAt: live.ts, text: "Current new" },
        ],
      },
    } as RuntimeDockItem;
    expect(buildRuntimeDockTickerEntries([projected], { a: [live] }).map((e) => e.text)).toEqual([
      "Current new",
    ]);
  });
});
