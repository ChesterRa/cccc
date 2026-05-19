import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "CapabilitiesTab.tsx"), "utf8");

describe("CapabilitiesTab request triggers", () => {
  it("uses the lightweight slash command state view for slash visibility refreshes", () => {
    expect(source).not.toContain('api.fetchGroupCapabilityState(String(groupId || "").trim(), "user", { noCache: true })');
    expect(source).not.toContain('api.fetchGroupCapabilityState(gid, "user", { noCache: true })');
    expect(source).toContain("api.fetchSlashCommandCapabilityState");
  });
});
