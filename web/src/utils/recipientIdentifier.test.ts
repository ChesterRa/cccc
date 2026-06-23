import { describe, expect, it } from "vitest";

import { formatRecipientIdentifier } from "./recipientIdentifier";

describe("formatRecipientIdentifier", () => {
  it("formats remote groups as compact single-line identifiers", () => {
    expect(formatRecipientIdentifier({
      kind: "remote_group",
      label: "D:\\dev\\temp",
      id: "g_16947475649e",
      accessLevel: "messages",
    })).toBe("D:\\dev\\temp (g_16947475649e remote/message only)");

    expect(formatRecipientIdentifier({
      kind: "remote_group",
      label: "SDK",
      id: "g_sdk",
      accessLevel: "read",
    })).toBe("SDK (g_sdk remote/read)");

    expect(formatRecipientIdentifier({
      kind: "remote_group",
      label: "Ops",
      id: "g_ops",
      accessLevel: "full",
    })).toBe("Ops (g_ops remote/full)");

    expect(formatRecipientIdentifier({
      kind: "remote_group",
      label: "Stale",
      id: "g_stale",
    })).toBe("Stale (g_stale remote/unknown)");
  });

  it("formats local actors with role and id when useful", () => {
    expect(formatRecipientIdentifier({
      kind: "actor",
      label: "P0",
      id: "p0",
      role: "peer",
    })).toBe("P0 (p0 local/peer)");

    expect(formatRecipientIdentifier({
      kind: "actor",
      label: "lead",
      id: "lead",
      role: "foreman",
    })).toBe("lead (local/foreman)");
  });

  it("formats local selector recipients without tool instructions", () => {
    const identifier = formatRecipientIdentifier({ kind: "selector", selector: "@foreman" });

    expect(identifier).toBe("@foreman (local selector)");
    expect(identifier).not.toContain("cccc_");
    expect(identifier.split("\n")).toHaveLength(1);
  });
});
