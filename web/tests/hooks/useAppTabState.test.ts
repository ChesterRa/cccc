import { describe, expect, it } from "vitest";

import { getRenderedActorIds, isChatViewportAtBottom } from "../../src/hooks/useAppTabState";

describe("useAppTabState", () => {
  it("treats only the real viewport position as at-bottom", () => {
    expect(isChatViewportAtBottom(1000, 920, 80)).toBe(true);
    expect(isChatViewportAtBottom(1000, 700, 80)).toBe(false);
  });

  it("respects the bottom threshold without any external follow-mode state", () => {
    expect(isChatViewportAtBottom(1000, 821, 80, 100)).toBe(true);
    expect(isChatViewportAtBottom(1000, 820, 80, 100)).toBe(false);
  });

  it("keeps the active runtime inspector mounted during a transient empty actor refresh", () => {
    expect(
      getRenderedActorIds({
        mountedActorIds: ["codex-1"],
        activeTab: "codex-1",
        runtimeActors: [],
      })
    ).toEqual(["codex-1"]);
  });

  it("drops inactive mounted actor tabs once they are no longer live", () => {
    expect(
      getRenderedActorIds({
        mountedActorIds: ["codex-1", "claude-1"],
        activeTab: "chat",
        runtimeActors: [{ id: "claude-1", role: "peer", runtime: "claude", runner: "pty" }],
      })
    ).toEqual(["claude-1"]);
  });

  it("does not render an unknown active runtime tab without a live actor or mounted cache", () => {
    expect(
      getRenderedActorIds({
        mountedActorIds: [],
        activeTab: "missing-actor",
        runtimeActors: [],
      })
    ).toEqual([]);
  });
});
