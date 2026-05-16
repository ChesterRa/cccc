import { describe, expect, it } from "vitest";

import { beginActorAction, endActorAction } from "../../src/hooks/actorActionInFlight";

describe("actor action in-flight guard", () => {
  it("rejects duplicate lifecycle actions for the same actor until released", () => {
    const inFlight = { current: new Set<string>() };

    expect(beginActorAction(inFlight, "actor-lifecycle:peer1")).toBe(true);
    expect(beginActorAction(inFlight, "actor-lifecycle:peer1")).toBe(false);

    endActorAction(inFlight, "actor-lifecycle:peer1");

    expect(beginActorAction(inFlight, "actor-lifecycle:peer1")).toBe(true);
  });
});
