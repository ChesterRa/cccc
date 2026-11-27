import { describe, expect, it } from "vite-plus/test";

import { actorAdvancedChanges } from "./actorConfigAdvancedModel";

describe("actorAdvancedChanges", () => {
  it("reports nothing while every advanced setting is at its default", () => {
    expect(
      actorAdvancedChanges({ commandOverridden: false, environmentText: " \n", capabilityIds: [] }),
    ).toEqual([]);
  });

  it("reports each changed setting with how many entries it holds", () => {
    expect(
      actorAdvancedChanges({
        commandOverridden: true,
        environmentText: "FIXTURE_A=1\nexport FIXTURE_B=2\n# note\nFIXTURE_A=3",
        capabilityIds: ["pack:one", " pack:two ", "pack:one", ""],
      }),
    ).toEqual([
      { kind: "command" },
      { kind: "environment", count: 2 },
      { kind: "capabilities", count: 2 },
    ]);
  });

  it("still reports an environment draft that does not parse yet", () => {
    expect(
      actorAdvancedChanges({
        commandOverridden: false,
        environmentText: "not a variable",
        capabilityIds: [],
      }),
    ).toEqual([{ kind: "environment", count: 1 }]);
  });
});
