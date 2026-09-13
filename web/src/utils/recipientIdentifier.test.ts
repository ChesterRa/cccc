import { describe, expect, it } from "vite-plus/test";

import { formatRecipientIdentifier } from "./recipientIdentifier";

describe("formatRecipientIdentifier", () => {
  it("formats local actors with role and id when useful", () => {
    expect(formatRecipientIdentifier({ kind: "actor", label: "P0", id: "p0", role: "peer" })).toBe(
      "P0 (p0 local/peer)",
    );

    expect(
      formatRecipientIdentifier({ kind: "actor", label: "lead", id: "lead", role: "foreman" }),
    ).toBe("lead (local/foreman)");
  });

  it("formats local selector recipients without tool instructions", () => {
    const identifier = formatRecipientIdentifier({ kind: "selector", selector: "@foreman" });

    expect(identifier).toBe("@foreman (local selector)");
    expect(identifier).not.toContain("cccc_");
    expect(identifier.split("\n")).toHaveLength(1);
  });
});
