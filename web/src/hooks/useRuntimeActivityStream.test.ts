import { describe, expect, it } from "vite-plus/test";
import { parseRuntimeActivityEvent } from "./useRuntimeActivityStream";

describe("runtime activity stream parser", () => {
  it("accepts the structured runtime activity contract", () => {
    expect(
      parseRuntimeActivityEvent({
        id: "event",
        group_id: "g1",
        actor_id: "peer",
        activity_id: "tool:1",
        status: "started",
      })?.id,
    ).toBe("event");
  });

  it("rejects events missing routing identity", () => {
    expect(parseRuntimeActivityEvent({ id: "event", status: "started" })).toBeNull();
  });
});
