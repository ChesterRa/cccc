import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "useChatTab.ts"), "utf8");

describe("useChatTab request triggers", () => {
  it("does not refresh slash commands from chat ledger event changes", () => {
    expect(source).not.toContain("latestFormalChatEventKey");
    expect(source).not.toMatch(/latestFormalChatEventKey[\s\S]*refreshSlashCommands/);
  });
});
