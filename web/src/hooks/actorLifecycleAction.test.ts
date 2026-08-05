import { describe, expect, it } from "vite-plus/test";

import { resolveActorLifecycleRunning } from "./actorLifecycleAction";

describe("resolveActorLifecycleRunning", () => {
  it("prefers the hydrated display state over a stale cached actor state", () => {
    expect(resolveActorLifecycleRunning({ running: false, enabled: true }, true)).toBe(true);
    expect(resolveActorLifecycleRunning({ running: true, enabled: true }, false)).toBe(false);
  });

  it("falls back to actor runtime and enabled state without an override", () => {
    expect(resolveActorLifecycleRunning({ running: true, enabled: false })).toBe(true);
    expect(resolveActorLifecycleRunning({ enabled: true })).toBe(true);
  });
});
