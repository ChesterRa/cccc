import { describe, expect, it } from "vite-plus/test";
import {
  getGroupControlVisual,
  getLaunchControlMode,
  resolveGroupControls,
} from "../../src/utils/groupControls";

describe("groupControls", () => {
  it("treats idle groups as activatable from the launch control", () => {
    expect(getLaunchControlMode("idle")).toBe("activate");
  });

  it("keeps the launch control visually active for running idle groups", () => {
    const visual = getGroupControlVisual("idle", "launch", "");
    expect(visual.active).toBe(true);
    expect(visual.pending).toBe(false);
    expect(visual.className).toContain("bg-emerald-700");
  });

  it("keeps pause active only for paused groups", () => {
    expect(getGroupControlVisual("idle", "pause", "").active).toBe(false);
    expect(getGroupControlVisual("paused", "pause", "").active).toBe(true);
  });

  it("does not mark pause as active for running groups", () => {
    expect(getGroupControlVisual("run", "pause", "").active).toBe(false);
  });

  it("disables pause only when there is no selectable running context", () => {
    expect(
      resolveGroupControls({ selectedGroupId: "g1", actorCount: 1, statusKey: "run", busy: "" })
        .pauseDisabled,
    ).toBe(false);
    expect(
      resolveGroupControls({ selectedGroupId: "g1", actorCount: 1, statusKey: "stop", busy: "" })
        .pauseDisabled,
    ).toBe(true);
  });
});
