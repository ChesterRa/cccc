import { describe, expect, it } from "vite-plus/test";
import { groupRunActions } from "../../src/utils/groupControls";
describe("Group run actions", () => {
  it("offers only meaningful transitions, including idle and paused without a process", () => {
    expect(groupRunActions("run")).toEqual(["pause", "stop"]);
    expect(groupRunActions("paused")).toEqual(["resume", "stop"]);
    expect(groupRunActions("idle")).toEqual(["resume", "pause", "stop"]);
    expect(groupRunActions("stop")).toEqual(["start"]);
  });
});
