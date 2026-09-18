import { describe, expect, it } from "vite-plus/test";
import { getGroupMenuControls } from "./groupControls";

describe("getGroupMenuControls", () => {
  it("offers pause while running, resume while paused or idle, launch otherwise", () => {
    expect(getGroupMenuControls("run").map((c) => c.control)).toEqual(["pause", "stop"]);
    expect(getGroupMenuControls("paused").map((c) => c.control)).toEqual(["activate", "stop"]);
    expect(getGroupMenuControls("idle").map((c) => c.control)).toEqual([
      "activate",
      "pause",
      "stop",
    ]);
    expect(getGroupMenuControls("stop").map((c) => c.control)).toEqual(["launch"]);
    expect(getGroupMenuControls(null).map((c) => c.control)).toEqual(["launch", "stop"]);
  });

  it("pairs each control with the header's former label", () => {
    expect(getGroupMenuControls("run")).toEqual([
      { control: "pause", labelKey: "pauseDelivery" },
      { control: "stop", labelKey: "stopAllAgents" },
    ]);
    expect(getGroupMenuControls("paused")[0]).toEqual({
      control: "activate",
      labelKey: "resumeDelivery",
    });
  });
});
