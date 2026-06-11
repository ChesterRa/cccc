import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

function readSource(relPath: string): string {
  const url = new URL(relPath, import.meta.url);
  return readFileSync(fileURLToPath(url), "utf-8");
}

describe("federation settings placement", () => {
  it("federation is a recognized global settings tab id", () => {
    expect(readSource("./types.ts")).toContain('| "federation"');
    expect(readSource("./settingsLastLocation.ts")).toContain('"federation"');
  });

  it("SettingsModal registers Federation as its own sidebar tab and view", () => {
    const src = readSource("../../SettingsModal.tsx");
    expect(src).toContain('id: "federation"');
    expect(src).toContain('activeTab === "federation"');
    expect(src).toContain("FederationRegistrationSection");
  });

  it("WebAccessTab no longer embeds the federation form", () => {
    const src = readSource("./WebAccessTab.tsx");
    expect(src).not.toContain("FederationRegistrationSection");
  });

  it("FederationRegistrationSection asks for a credential reference, not a token", () => {
    const src = readSource("./FederationRegistrationSection.tsx");
    expect(src).toContain("Credential reference");
    expect(src).not.toContain("Credential / token");
  });
});
