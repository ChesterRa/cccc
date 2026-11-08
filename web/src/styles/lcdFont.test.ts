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

it("changes only message text for proportional while keeping saved mono and pixel choices", () => {
  const root = document.documentElement;
  const previousStyle = root.getAttribute("data-ui-style");
  const previousBody = root.getAttribute("data-lcd-body");
  const style = document.createElement("style");
  const host = document.createElement("div");
  style.textContent = css;
  const markup = `
    <div data-lcd-scope><div data-chat-work-surface>
      <span data-message-quote-text>Quote</span>
      <div data-message-content>
        <div class="markdown-body"><p>Message <code>inline</code></p><pre>block</pre></div>
      </div>
      <span data-message-insight-text>Insight</span>
      <button>Control</button>
    </div></div>`;
  host.innerHTML = markup;
  root.setAttribute("data-ui-style", "lcd");
  root.setAttribute("data-lcd-body", "proportional");
  document.head.append(style);
  document.body.append(host);
  try {
    const font = (selector: string) => getComputedStyle(host.querySelector(selector)!).fontFamily;
    for (const selector of ["p", "[data-message-quote-text]", "[data-message-insight-text]"]) {
      expect(font(selector)).toContain("IBM Plex Sans");
    }
    for (const selector of ["code", "pre", "button"]) {
      expect(font(selector)).toContain("IBM Plex Mono");
    }
    root.setAttribute("data-lcd-body", "mono");
    host.innerHTML = markup;
    expect(font("p")).toContain("IBM Plex Mono");
    root.setAttribute("data-lcd-body", "pixel");
    host.innerHTML = markup;
    expect(font("p")).toContain("RoundPix 16");
    expect(font("code")).toContain("IBM Plex Mono");
  } finally {
    style.remove();
    host.remove();
    if (previousStyle === null) root.removeAttribute("data-ui-style");
    else root.setAttribute("data-ui-style", previousStyle);
    if (previousBody === null) root.removeAttribute("data-lcd-body");
    else root.setAttribute("data-lcd-body", previousBody);
  }
});
