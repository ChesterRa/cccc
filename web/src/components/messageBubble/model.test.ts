import { describe, expect, it } from "vitest";

import { buildToLabel } from "./model";

function labelFor(recipients: string[] | undefined): string {
  return buildToLabel({
    hasDestination: false,
    dstGroupId: "",
    dstTo: [],
    groupLabelById: {},
    recipients,
    displayNameMap: new Map(),
  });
}

describe("buildToLabel", () => {
  it("falls back to @foreman when recipients are empty", () => {
    expect(labelFor([])).toBe("@foreman");
  });

  it("falls back to @foreman when recipients are missing", () => {
    expect(labelFor(undefined)).toBe("@foreman");
  });

  it("keeps explicit @all recipients", () => {
    expect(labelFor(["@all"])).toBe("@all");
  });
});
