import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";

describe("virtual scroll message measurements", () => {
  it("binds row estimates to the current render's message collection", () => {
    const source = readFileSync(new URL("./useVirtualScrollState.ts", import.meta.url), "utf8");
    expect(source).toContain("const message = messages[index];");
    expect(source).toMatch(/\[messages,\s*viewKey\],?\s*\)/);
    expect(source).not.toContain("messagesRef");
  });
});
