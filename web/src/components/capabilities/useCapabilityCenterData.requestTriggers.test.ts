import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";

const source = readFileSync(new URL("./useCapabilityCenterData.ts", import.meta.url), "utf8");

describe("capability center request triggers", () => {
  it("keeps group state out of overview search and pagination reloads", () => {
    const overviewStart = source.indexOf("const loadOverview");
    const stateStart = source.indexOf("const loadState");
    const overview = source.slice(overviewStart, stateStart);

    expect(overview).toContain("fetchCapabilityOverview");
    expect(overview).not.toContain("fetchGroupCapabilityState");
  });

  it("loads group state through its own group-scoped callback", () => {
    expect(source).toContain("fetchGroupCapabilityState(groupId");
    expect(source).toContain("[failedStateMessage, groupId]");
  });

  it("ignores a stale state response after the active group changes", () => {
    expect(source).toContain("const stateRequestSequence = useRef(0)");
    expect(source).toContain("const sequence = ++stateRequestSequence.current");
    expect(source).toContain("if (sequence !== stateRequestSequence.current) return");
  });
});
