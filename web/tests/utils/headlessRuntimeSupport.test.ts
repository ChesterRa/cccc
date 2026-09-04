import { describe, expect, it } from "vite-plus/test";

import {
  getEffectiveActorRunner,
  hasManagedRuntimeOutput,
  isHeadlessActorRunner,
  normalizeActorRunner,
} from "../../src/utils/headlessRuntimeSupport";

describe("normalizeActorRunner", () => {
  it("normalizes headless and defaults unknown values to pty", () => {
    expect(normalizeActorRunner("headless")).toBe("headless");
    expect(normalizeActorRunner(" HEADLESS ")).toBe("headless");
    expect(normalizeActorRunner("pty")).toBe("pty");
    expect(normalizeActorRunner("other")).toBe("pty");
    expect(normalizeActorRunner(undefined)).toBe("pty");
  });
});

describe("hasManagedRuntimeOutput", () => {
  it("uses the same predicate for legacy headless and managed-session actors", () => {
    expect(hasManagedRuntimeOutput({ runner: "headless" })).toBe(true);
    expect(
      hasManagedRuntimeOutput({ runner: "pty", runtime_state_source: "managed_session" }),
    ).toBe(true);
    expect(hasManagedRuntimeOutput({ runtime_state_source: "app_server" })).toBe(true);
    expect(hasManagedRuntimeOutput({ runner: "pty" })).toBe(false);
  });
});

describe("getEffectiveActorRunner", () => {
  it("prefers runner_effective over runner", () => {
    expect(getEffectiveActorRunner({ runner: "pty", runner_effective: "headless" })).toBe(
      "headless",
    );
  });

  it("lets callers check headless mode from partial actor objects", () => {
    expect(isHeadlessActorRunner({ runner: "headless" })).toBe(true);
    expect(isHeadlessActorRunner({ runner: "pty", runner_effective: "pty" })).toBe(false);
  });
});
