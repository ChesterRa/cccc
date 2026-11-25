// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";

const css = readFileSync(fileURLToPath(new NodeURL("./lcd.css", import.meta.url)), "utf8");

function collectStyleRules(list: CSSRuleList): CSSStyleRule[] {
  const out: CSSStyleRule[] = [];
  for (const rule of [...list]) {
    if ("selectorText" in rule) out.push(rule as CSSStyleRule);
    if ("cssRules" in rule) out.push(...collectStyleRules((rule as CSSGroupingRule).cssRules));
  }
  return out;
}

it("reverses the active sidebar row's menu trigger back to paper and ink when open or hovered", () => {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);
  try {
    const rules = collectStyleRules(style.sheet!.cssRules);
    const openRule = rules.find(
      (rule) =>
        rule.selectorText.includes(".glass-group-item-active") &&
        rule.selectorText.includes('button[aria-haspopup="menu"][aria-expanded="true"]'),
    );
    const hoverRule = rules.find(
      (rule) =>
        rule.selectorText.includes(".glass-group-item-active") &&
        rule.selectorText.includes('button[aria-haspopup="menu"]:hover'),
    );
    expect(openRule?.parentRule).toBeNull();
    expect(openRule?.style.getPropertyValue("background")).toBe("var(--lcd-sel-fg)");
    expect(openRule?.style.getPropertyValue("color")).toBe("var(--lcd-sel-bg)");
    expect(hoverRule?.style.getPropertyValue("background")).toBe("var(--lcd-sel-fg)");
    expect(hoverRule?.style.getPropertyValue("color")).toBe("var(--lcd-sel-bg)");
    // Hover only where the pointer can hover, so a tap leaves no sticky reversed key.
    expect((hoverRule?.parentRule as CSSMediaRule | null)?.conditionText).toBe("(hover: hover)");
  } finally {
    style.remove();
  }
});
