// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
const css = readFileSync(fileURLToPath(new NodeURL("./lcd.css", import.meta.url)), "utf8");

// Only the run triangle takes a colour, defined for both themes; every other state is
// told apart by its shape alone.
it("colours the run symbol alone, in both themes", () => {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);
  try {
    const rules = [...style.sheet!.cssRules].filter(
      (item): item is CSSStyleRule => "selectorText" in item,
    );
    const rule = (selector: string) => rules.find((item) => item.selectorText === selector);
    expect(rule(':root[data-ui-style="lcd"]')?.style.getPropertyValue("--lcd-run")).toBeTruthy();
    expect(
      rule(':root[data-ui-style="lcd"].dark')?.style.getPropertyValue("--lcd-run"),
    ).toBeTruthy();
    const coloured = rules.filter(
      (item) => item.selectorText.includes(".lcd-group-symbol") && item.style.color,
    );
    expect(coloured.map((item) => [item.selectorText, item.style.color])).toEqual([
      [':root[data-ui-style="lcd"] .lcd-group-symbol[data-lcd-shape="run"]', "var(--lcd-run)"],
    ]);
  } finally {
    style.remove();
  }
});
