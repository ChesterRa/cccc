import { describe, expect, it } from "vitest";

import {
  buildCapabilityCenterUrl,
  capabilityCenterGroupIdFromSearch,
  isCapabilityCenterPath,
} from "../../src/components/capabilities/capabilityCenterRoute";

describe("CapabilityCenter route", () => {
  it("detects the standalone capability page path", () => {
    expect(isCapabilityCenterPath("/ui/capabilities")).toBe(true);
    expect(isCapabilityCenterPath("/ui/capabilities/")).toBe(true);
    expect(isCapabilityCenterPath("/ui/")).toBe(false);
  });

  it("carries the current group id into the new tab URL", () => {
    expect(buildCapabilityCenterUrl(" g1 ", "http://127.0.0.1:5173")).toBe("/ui/capabilities?group_id=g1");
    expect(capabilityCenterGroupIdFromSearch("?group_id=g1")).toBe("g1");
  });
});
