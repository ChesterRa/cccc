import { describe, expect, it } from "vitest";

import {
  getEffectiveActorRunner,
  isHeadlessActorRunner,
  normalizeActorRunner,
  supportsStandardWebHeadlessRuntime,
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

describe("getEffectiveActorRunner", () => {
  it("prefers runner_effective over runner", () => {
    expect(getEffectiveActorRunner({ runner: "pty", runner_effective: "headless" })).toBe("headless");
  });

  it("lets callers check headless mode from partial actor objects", () => {
    expect(isHeadlessActorRunner({ runner: "headless" })).toBe(true);
    expect(isHeadlessActorRunner({ runner: "pty", runner_effective: "pty" })).toBe(false);
  });
});

describe("supportsStandardWebHeadlessRuntime", () => {
  it("allows the standard-web headless runtime whitelist", () => {
    expect(supportsStandardWebHeadlessRuntime("codex")).toBe(true);
    expect(supportsStandardWebHeadlessRuntime(" claude ")).toBe(true);
  });

  it("rejects unsupported runtimes", () => {
    expect(supportsStandardWebHeadlessRuntime("custom")).toBe(false);
    expect(supportsStandardWebHeadlessRuntime("gemini")).toBe(false);
    expect(supportsStandardWebHeadlessRuntime("grok")).toBe(false);
    expect(supportsStandardWebHeadlessRuntime("")).toBe(false);
  });
});
