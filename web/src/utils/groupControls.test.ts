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

  it("keeps Stop for an Offline Group, whose recorded lifecycle is not stopped", () => {
    expect(getGroupMenuControls("offline")).toEqual([
      { control: "launch", labelKey: "launchAllAgents" },
      { control: "stop", labelKey: "stopAllAgents" },
    ]);
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
