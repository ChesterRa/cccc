import { describe, expect, it } from "vitest";

import { shouldFetchStoppedTerminalTail } from "../../src/components/AgentTab";

describe("AgentTab stopped terminal tail model", () => {
  it("does not fetch stopped terminal output while the actor has an in-flight lifecycle action", () => {
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: false,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: true,
      }),
    ).toBe(false);
  });

  it("fetches stopped terminal output only for an activated non-headless stopped actor", () => {
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: false,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: false,
      }),
    ).toBe(true);
    expect(
      shouldFetchStoppedTerminalTail({
        activated: true,
        isRunning: true,
        isHeadless: false,
        groupId: "g1",
        actorId: "Development",
        isActorBusy: false,
      }),
    ).toBe(false);
  });
});
