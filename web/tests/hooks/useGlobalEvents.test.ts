import { describe, expect, it } from "vitest";
import {
  getGlobalEventGroupId,
  shouldRefreshCapabilitiesAfterGlobalEvent,
  shouldRefreshGroupBridgePairingAfterGlobalEvent,
  shouldRefreshGroupBridgePairingAfterGlobalEventsOpen,
  shouldKeepGlobalEventsConnected,
  shouldRefreshActorsAfterGlobalEvent,
  shouldRefreshGroupsAfterGlobalEventsOpen,
} from "../../src/hooks/useGlobalEvents";

describe("useGlobalEvents open refresh policy", () => {
  it("requires catch-up refresh on the first successful open", () => {
    expect(shouldRefreshGroupsAfterGlobalEventsOpen(false)).toBe(true);
  });

  it("requires catch-up refresh on reconnects too", () => {
    expect(shouldRefreshGroupsAfterGlobalEventsOpen(true)).toBe(true);
  });

  it("requires Group Bridge pairing catch-up refresh on global event stream open", () => {
    expect(shouldRefreshGroupBridgePairingAfterGlobalEventsOpen(false)).toBe(true);
    expect(shouldRefreshGroupBridgePairingAfterGlobalEventsOpen(true)).toBe(true);
  });

  it("releases the global SSE connection while the tab is hidden", () => {
    expect(shouldKeepGlobalEventsConnected(false)).toBe(true);
    expect(shouldKeepGlobalEventsConnected(true)).toBe(false);
  });

  it("extracts group id from top-level global events", () => {
    expect(getGlobalEventGroupId({ kind: "actor.stop", group_id: "g-demo" })).toBe("g-demo");
  });

  it("extracts group id from nested event data as a fallback", () => {
    expect(getGlobalEventGroupId({ kind: "group.state_changed", data: { group_id: "g-demo" } })).toBe("g-demo");
  });

  it("refreshes selected actors for matching lifecycle events", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.stop", group_id: "g-demo", data: { actor_id: "peer-1" } },
        "g-demo",
      ),
    ).toBe(true);
  });

  it("refreshes selected actors for actor removal events too", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.remove", group_id: "g-demo", data: { actor_id: "peer-1" } },
        "g-demo",
      ),
    ).toBe(true);
  });

  it("ignores lifecycle events for other groups", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "actor.stop", group_id: "g-other", data: { actor_id: "peer-1" } },
        "g-demo",
      ),
    ).toBe(false);
  });

  it("ignores non-lifecycle global events for actor refresh", () => {
    expect(
      shouldRefreshActorsAfterGlobalEvent(
        { kind: "group.updated", group_id: "g-demo" },
        "g-demo",
      ),
    ).toBe(false);
  });

  it("refreshes selected capability state after capability changes", () => {
    expect(
      shouldRefreshCapabilitiesAfterGlobalEvent(
        { kind: "capability.changed", data: { group_id: "g-demo", capability_id: "skill:demo" } },
        "g-demo",
      ),
    ).toBe(true);
  });

  it("ignores capability changes for other groups", () => {
    expect(
      shouldRefreshCapabilitiesAfterGlobalEvent(
        { kind: "capability.changed", data: { group_id: "g-other", capability_id: "skill:demo" } },
        "g-demo",
      ),
    ).toBe(false);
  });

  it("refreshes selected Group Bridge pairing state after pairing changes", () => {
    expect(
      shouldRefreshGroupBridgePairingAfterGlobalEvent(
        { kind: "group_bridge.pairing.request_created", data: { group_id: "g-demo", request_id: "preq_1" } },
        "g-demo",
      ),
    ).toBe(true);
  });

  it("refreshes selected Group Bridge pairing state after outbound approval creates a local active route", () => {
    expect(
      shouldRefreshGroupBridgePairingAfterGlobalEvent(
        {
          kind: "group_bridge.pairing.outbound_approved",
          data: { group_id: "g-demo", trust_id: "ptrust_1", registration_id: "reg_1" },
        },
        "g-demo",
      ),
    ).toBe(true);
  });

  it("ignores Group Bridge pairing changes for other groups", () => {
    expect(
      shouldRefreshGroupBridgePairingAfterGlobalEvent(
        { kind: "group_bridge.pairing.request_created", data: { group_id: "g-other", request_id: "preq_1" } },
        "g-demo",
      ),
    ).toBe(false);
  });
});
