// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
const css = readFileSync(fileURLToPath(new NodeURL("./lcd.css", import.meta.url)), "utf8");

it("prefers Japanese glyphs for Japanese LCD body text in both font modes", () => {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);
  try {
    const rules = [...style.sheet!.cssRules];
    const rule = (selector: string) =>
      rules.find(
        (item): item is CSSStyleRule => "selectorText" in item && item.selectorText === selector,
      );
    const mono =
      rule(':root[data-ui-style="lcd"]:lang(ja)')?.style.getPropertyValue("--lcd-font-cjk") || "";
    expect(mono.indexOf("CJK JP")).toBeGreaterThanOrEqual(0);
    expect(mono.indexOf("CJK JP")).toBeLessThan(mono.indexOf("CJK SC"));
    const pixel =
      rule(':root[data-ui-style="lcd"][data-lcd-body="pixel"]:lang(ja)')?.style.getPropertyValue(
        "--lcd-font-body",
      ) || "";
    expect(pixel.indexOf("Fusion Pixel 12 JA Body")).toBeGreaterThanOrEqual(0);
    expect(pixel.indexOf("Fusion Pixel 12 JA Body")).toBeLessThan(pixel.indexOf("RoundPix 16"));
    const face = rules.find(
      (item): item is CSSFontFaceRule =>
        item instanceof CSSFontFaceRule &&
        item.style.getPropertyValue("font-family").includes("Fusion Pixel 12 JA Body"),
    );
    expect(face?.style.getPropertyValue("unicode-range")).toContain("2E80-9FFF");
  } finally {
    style.remove();
  }
});
