// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";

const css = readFileSync(fileURLToPath(new NodeURL("./lcd.css", import.meta.url)), "utf8");

it("caps the Pixel header title to the 24px grid on phones without changing desktop sizing", () => {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);
  try {
    const selector = ':root[data-ui-style="lcd"] [data-lcd-scope] [data-group-title]';
    const topRules = [...style.sheet!.cssRules].filter(
      (item): item is CSSStyleRule => "selectorText" in item && item.selectorText === selector,
    );
    expect(topRules).toHaveLength(1);
    expect(topRules[0]!.style.fontSize).toBe("1.5rem");
    expect(topRules[0]!.style.lineHeight).toBe("1.75rem");

    const mediaRules = [...style.sheet!.cssRules].filter(
      (item): item is CSSMediaRule =>
        "media" in item && item.media.mediaText.includes("max-width: 767px"),
    );
    const mobileTitleRule = mediaRules
      .flatMap((media) => [...media.cssRules])
      .find(
        (item): item is CSSStyleRule => "selectorText" in item && item.selectorText === selector,
      );
    expect(mobileTitleRule).toBeDefined();
    // happy-dom's CSSStyleDeclaration strips CSS math functions like min(), so check the rule block.
    const mediaBlock = css.match(
      /@media\s*\(max-width:\s*767px\)\s*\{\s*:root\[data-ui-style="lcd"\]\s+\[data-lcd-scope\]\s+\[data-group-title\]\s*\{([^}]*)\}\s*\}/,
    );
    expect(mediaBlock?.[1]).toMatch(/font-size:\s*min\(1\.5rem,\s*24px\)/);
    expect(mediaBlock?.[1]).toMatch(/line-height:\s*min\(1\.75rem,\s*28px\)/);
  } finally {
    style.remove();
  }
});
