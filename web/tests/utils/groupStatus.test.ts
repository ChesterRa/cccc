import { describe, expect, it } from "vite-plus/test";

import {
  getGroupRuntimeStatus,
  getGroupStatus,
  getGroupStatusFromSource,
} from "../../src/utils/groupStatus";
import {
  QUIET_RUN_INDICATOR_DOT_CLASS,
  STOPPED_INDICATOR_DOT_CLASS,
} from "../../src/utils/statusIndicators";

describe("getGroupStatus", () => {
  it.each([
    [false, "paused", "paused"],
    [true, "paused", "paused"],
    [false, "stopped", "stop"],
    [true, "stopped", "stop"],
    [false, "active", "offline"],
    [false, "idle", "offline"],
    [false, undefined, "offline"],
    [true, "idle", "idle"],
    [true, "active", "run"],
    [true, undefined, "run"],
  ] as const)("running=%s state=%s is %s", (running, state, key) => {
    expect(getGroupStatus(running, state).key).toBe(key);
  });

  it("maps quiet running to the shared quiet run indicator", () => {
    expect(getGroupStatus(true, "active").dotClass).toBe(QUIET_RUN_INDICATOR_DOT_CLASS);
  });

  it("shows Stopped and Offline on the muted stopped indicator, since nothing runs in either", () => {
    expect(getGroupStatus(false, "stopped").dotClass).toBe(STOPPED_INDICATOR_DOT_CLASS);
    expect(getGroupStatus(false, "active").dotClass).toBe(STOPPED_INDICATOR_DOT_CLASS);
  });
});

describe("getGroupStatusFromSource", () => {
  it("prefers the daemon's runtime status over the legacy Group fields", () => {
    expect(
      getGroupStatusFromSource({
        running: true,
        state: "active",
        runtime_status: {
          lifecycle_state: "active",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      }).key,
    ).toBe("offline");
    expect(
      getGroupStatusFromSource({
        running: false,
        state: "active",
        runtime_status: {
          lifecycle_state: "stopped",
          runtime_running: false,
          running_actor_count: 0,
          has_running_foreman: false,
        },
      }).key,
    ).toBe("stop");
  });

  it("falls back to the Group fields when no runtime status is present", () => {
    expect(getGroupStatusFromSource({ running: false, state: "active" }).key).toBe("offline");
    expect(getGroupStatusFromSource({ running: true, state: "idle" }).key).toBe("idle");
    expect(getGroupStatusFromSource(null).key).toBe("offline");
  });

  it.each([
    ["active", "unknown"],
    ["idle", "unknown"],
    ["paused", "paused"],
    ["stopped", "stop"],
  ] as const)(
    "shows a Group the daemon could not observe in lifecycle %s as %s",
    (lifecycle, key) => {
      const source = {
        running: true,
        state: "active" as const,
        runtime_status: { lifecycle_state: lifecycle, available: false },
      };
      expect(getGroupStatusFromSource(source).key).toBe(key);
      // Only the recorded lifecycle survives; no running state is made up.
      expect(getGroupRuntimeStatus(source)).toEqual({
        lifecycle_state: lifecycle,
        available: false,
      });
    },
  );
});
