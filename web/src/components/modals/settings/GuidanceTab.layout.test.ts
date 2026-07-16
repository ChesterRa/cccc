import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vite-plus/test";

const source = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "GuidanceTab.tsx"),
  "utf8",
);

describe("GuidanceTab expanded editor layout", () => {
  it("keeps the expanded editor inside a clipped flex body", () => {
    expect(source).toContain('className="min-h-0 flex-1 overflow-hidden p-4 sm:p-6 lg:p-7"');
    expect(source).toContain('expanded ? "min-h-0 flex-1 resize-none" : "min-h-[320px] resize-y"');
  });

  it("keeps expanded actions outside the resizable editor area", () => {
    expect(source).toMatch(
      /expanded\s*\?\s*"mt-4 flex shrink-0 flex-wrap items-center gap-2 border-t/,
    );
  });

  it("lets the view switcher stack without compressing its labels", () => {
    expect(source).toContain("flex shrink-0 flex-col gap-3 sm:flex-row sm:items-center");
    expect(source).toContain("w-full shrink-0 sm:w-auto");
  });

  it("scrolls the single-column workspace and clips only the desktop split view", () => {
    expect(source).toContain("overflow-y-auto xl:overflow-hidden xl:grid-cols");
    expect(source).toContain("min-h-[360px] flex flex-col overflow-hidden xl:min-h-0");
  });
});
