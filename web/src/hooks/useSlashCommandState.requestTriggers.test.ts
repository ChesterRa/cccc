import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "useSlashCommandState.ts"), "utf8");

describe("useSlashCommandState request triggers", () => {
  it("does not refresh slash commands when the PWA window regains focus", () => {
    expect(source).not.toContain('window.addEventListener("focus"');
    expect(source).not.toContain('window.removeEventListener("focus"');
  });
});
