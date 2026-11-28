// @vitest-environment happy-dom
import { afterEach, expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
const css = readFileSync(fileURLToPath(new NodeURL("./previewTheme.css", import.meta.url)), "utf8");

// Elements the preview stylesheet restyles, as they appear in the app.
const PROBES = `
  <span class="lcd-group-symbol"></span>
  <button data-group-run-controls><span data-connection-state="connected"></span><span></span></button>
  <div role="dialog"><button class="border-[var(--color-text-primary)].bg-[var(--color-text-primary)]"></button></div>
  <button data-actor-mode-button="selected"></button>
  <div data-lcd-scope><div data-segmented-choice><span role="radio" aria-checked="true"></span></div></div>
  <button data-brand-name></button>
  <div class="lcd-row" data-lcd-user><div class="lcd-row-content"></div></div>
  <span data-pending-state></span>
  <div class="chat-dock-fade"></div>
  <div class="glass-modal glass-panel"></div>
  <h1></h1><code></code><input />
  <button class="lcd-member" data-tone="stopped"><span class="lcd-tile" data-probe="stopped"></span></button>
  <button class="lcd-member" data-tone="idle"><span class="lcd-tile" data-probe="idle"></span></button>
  <div data-message-row="true" class="pb-4" data-probe="message-row">
    <div class="lcd-row" data-probe="lcd-row"></div>
  </div>
  <div role="dialog">
    <button aria-pressed="true" class="border-[var(--color-text-primary)] bg-[var(--color-text-primary)]" data-probe="picked"></button>
  </div>
  <span class="lcd-member-mark" data-mark="mail" data-probe="mail"></span>
  <div data-group-pending-slot data-probe="pending-slot"></div>
  <div data-taskboard-filters><button aria-pressed="true" data-probe="board-filter"></button></div>
  <button data-mode-choice aria-pressed="true" data-probe="mode-on"></button>
  <button data-mode-choice aria-pressed="false" data-probe="mode-off"></button>
  <span class="lcd-row-route" data-probe="route"></span>
  <form data-pending-reply>
    <div class="lcd-ask-other" data-probe="ask-row">
      <label></label>
      <div class="lcd-ask-action"><button data-ask-send data-probe="send"><span data-ask-send-mark></span></button></div>
    </div>
  </form>
`;

const root = document.documentElement;
const previous = {
  style: root.getAttribute("data-ui-style"),
  dark: root.classList.contains("dark"),
};

afterEach(() => {
  document.body.innerHTML = "";
  if (previous.style === null) root.removeAttribute("data-ui-style");
  else root.setAttribute("data-ui-style", previous.style);
  root.classList.toggle("dark", previous.dark);
});

function mount(style: string, theme: "dark" | "light" = "dark"): CSSStyleRule[] {
  root.setAttribute("data-ui-style", style);
  root.classList.toggle("dark", theme === "dark");
  document.body.innerHTML = PROBES;
  const sheet = document.createElement("style");
  sheet.textContent = css;
  document.head.append(sheet);
  const rules = [...sheet.sheet!.cssRules].filter(
    (rule): rule is CSSStyleRule => "selectorText" in rule,
  );
  sheet.remove();
  return rules;
}

/** The values the root's matching rules give a custom property, in source order. */
function token(rules: CSSStyleRule[], name: string): string[] {
  return rules
    .filter((rule) => root.matches(rule.selectorText))
    .map((rule) => rule.style.getPropertyValue(name).trim())
    .filter(Boolean);
}

function matchingRules(style: string): string[] {
  root.setAttribute("data-ui-style", style);
  root.classList.add("dark");
  document.body.setAttribute("data-lcd-scope", "");
  document.body.innerHTML = PROBES;
  const sheet = document.createElement("style");
  sheet.textContent = css;
  document.head.append(sheet);
  try {
    const elements = [root, document.body, ...document.body.querySelectorAll("*")];
    return [...sheet.sheet!.cssRules]
      .filter((rule): rule is CSSStyleRule => "selectorText" in rule)
      .filter((rule) => elements.some((element) => element.matches(rule.selectorText)))
      .map((rule) => rule.selectorText);
  } finally {
    sheet.remove();
    document.body.removeAttribute("data-lcd-scope");
  }
}

it("dims stopped member tiles in preview, leaving the others lit", () => {
  const rules = mount("preview");
  const opacityOf = (probe: string) => {
    const tile = document.querySelector(`[data-probe="${probe}"]`)!;
    return rules
      .filter((rule) => tile.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue("opacity"))
      .filter(Boolean);
  };
  expect(opacityOf("stopped")).toEqual(["0.45"]);
  expect(opacityOf("idle")).toEqual([]);
});

it("gives the pending-reply card a stacked last row and a quiet send key", () => {
  const rules = mount("preview");
  const declared = (probe: string, property: string) => {
    const element = document.querySelector(`[data-probe="${probe}"]`)!;
    return rules
      .filter((rule) => element.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(property))
      .filter(Boolean);
  };
  expect(declared("ask-row", "flex-direction")).toEqual(["column"]);
  // Translucent in either theme, not the solid primary block the shared button paints.
  expect(declared("send", "background")).toEqual(["var(--preview-quiet-key-bg)"]);
  expect(token(rules, "--preview-quiet-key-bg")).toEqual(["rgba(255, 255, 255, 0.1)"]);
  expect(token(mount("preview", "light"), "--preview-quiet-key-bg")).toEqual([
    "rgba(17, 18, 20, 0.05)",
  ]);
});

it("tightens the space between consecutive messages", () => {
  const rules = mount("preview");
  const declared = (probe: string, property: string) => {
    const element = document.querySelector(`[data-probe="${probe}"]`)!;
    return rules
      .filter((rule) => element.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(property))
      .filter(Boolean);
  };
  expect(declared("message-row", "padding-bottom")).toEqual(["0.25rem"]);
  expect(declared("lcd-row", "padding-block")).toEqual(["0.25rem"]);
});

it("uses neutral, not blue-grey, sidebar selection in dark preview", () => {
  const rules = mount("preview");
  const active = rules
    .filter((rule) => root.matches(rule.selectorText))
    .map((rule) => rule.style.getPropertyValue("--color-sidebar-item-active").trim())
    .filter(Boolean);
  expect(active).toEqual(["rgba(255, 255, 255, 0.08)"]);
});

it("keeps one solid accent: soft-grey primary, translucent selected toggles, muted counts", () => {
  const rules = mount("preview");
  const declared = (element: Element, property: string) =>
    rules
      .filter((rule) => element.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(property).trim())
      .filter(Boolean);
  const probe = (name: string) => document.querySelector(`[data-probe="${name}"]`)!;
  // Primary actions read --primary, which follows the accent: solid, not pure white.
  expect(declared(root, "--color-accent-primary")).toEqual(["#d9dbe0"]);
  expect(declared(root, "--primary-foreground")).toEqual(["#0f1013"]);
  // A white self-painted selected toggle drops to the translucent selected tone.
  expect(declared(probe("picked"), "background")).toEqual(["var(--preview-selected-bg)"]);
  // Unread counts are a muted chip, not saturated amber.
  expect(declared(probe("mail"), "background")).toEqual(["var(--preview-mark-bg)"]);
  expect(token(rules, "--preview-mark-bg")).toEqual(["#2b2822"]);
});

it("marks a selected mode choice with a faint lift and one hairline, not a bevelled slab", () => {
  const rules = mount("preview");
  const declared = (name: string, property: string) => {
    const element = document.querySelector(`[data-probe="${name}"]`)!;
    return rules
      .filter((rule) => element.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(property).trim())
      .filter(Boolean);
  };
  expect(declared("mode-on", "background")).toEqual(["var(--preview-selected-bg)"]);
  expect(declared("mode-on", "box-shadow")).toEqual(["none"]);
  expect(declared("mode-off", "background")).toEqual(["transparent"]);
  const tokens = rules
    .filter((rule) => root.matches(rule.selectorText))
    .map((rule) => rule.style.getPropertyValue("--preview-selected-bg").trim())
    .filter(Boolean);
  expect(tokens).toEqual(["rgba(255, 255, 255, 0.05)"]);
});

it("gives a Group row's empty pending column to its name", () => {
  const rules = mount("preview");
  const slot = document.querySelector('[data-probe="pending-slot"]')!;
  const display = rules
    .filter((rule) => slot.matches(rule.selectorText))
    .map((rule) => rule.style.getPropertyValue("display"));
  expect(display).toEqual(["none"]);
});

it("uses neutral hairlines and sets message metadata a step back", () => {
  const rules = mount("preview");
  const tokens = (name: string) =>
    rules
      .filter((rule) => root.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(name).trim())
      .filter(Boolean);
  expect(tokens("--glass-border")).toEqual(["rgba(255, 255, 255, 0.1)"]);
  expect(tokens("--glass-border-subtle")).toEqual(["rgba(255, 255, 255, 0.06)"]);
  const route = document.querySelector('[data-probe="route"]')!;
  expect(
    rules
      .filter((rule) => route.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue("color")),
  ).toEqual(["var(--color-text-tertiary)"]);
});

it("gives light preview its own paper palette, separate from the dark one", () => {
  const light = mount("preview", "light");
  // Near-black ink as the one solid accent, on a soft off-white page with white cards.
  expect(token(light, "--color-accent-primary")).toEqual(["#111214"]);
  expect(token(light, "--primary-foreground")).toEqual(["#ffffff"]);
  expect(token(light, "--color-body-bg")).toEqual(["#f3f3f5"]);
  expect(token(light, "--glass-bubble-bg")).toEqual(["#ffffff"]);
  expect(token(light, "--color-text-primary")).toEqual(["#111214"]);
  expect(token(light, "--preview-selected-text")).toEqual(["#111214"]);
  // White surfaces are lifted by a faint shadow; dark stays flat.
  expect(token(light, "--glass-bubble-shadow")).toEqual(["0 1px 2px rgba(17, 18, 20, 0.04)"]);
  // One neutral ink for every line: none of the base theme's slate blue is left.
  const lightRule = light.find((rule) => rule.selectorText.includes(":not(.dark)"))!;
  expect(lightRule.style.cssText).not.toMatch(/rgba?\(15[ ,]+23[ ,]+42/);
  // Hairlines as faint as in dark.
  expect(token(light, "--glass-border-subtle")).toEqual(["rgba(17, 18, 20, 0.07)"]);
  // Dark keeps its own values; neither theme inherits the other's.
  const dark = mount("preview", "dark");
  expect(token(dark, "--color-body-bg")).toEqual(["#0a0b0d"]);
  expect(token(dark, "--preview-card-shadow")).toEqual(["none"]);
  expect(token(dark, "--preview-selected-text")).toEqual(["#ffffff"]);
});

it("raises a chosen segment as a white key in light and keeps board filters off the solid accent", () => {
  const light = mount("preview", "light");
  const declared = (rules: CSSStyleRule[], name: string, property: string) => {
    const element = document.querySelector(`[data-probe="${name}"]`)!;
    return rules
      .filter((rule) => element.matches(rule.selectorText))
      .map((rule) => rule.style.getPropertyValue(property).trim())
      .filter(Boolean);
  };
  // The switch rule is one long :is() list this DOM cannot match, so the tokens it reads are
  // checked instead.
  expect(token(light, "--preview-segment-bg")).toEqual(["#ffffff"]);
  expect(token(light, "--preview-segment-shadow")).toEqual(["0 1px 2px rgba(17, 18, 20, 0.12)"]);
  // The chosen board filter reads as selected, leaving "New task" the one solid key.
  expect(declared(light, "board-filter", "background")).toEqual(["var(--preview-selected-bg)"]);
  const dark = mount("preview", "dark");
  expect(token(dark, "--preview-segment-shadow")).toEqual(["none"]);
});

it("restyles the preview look", () => {
  expect(matchingRules("preview").length).toBeGreaterThan(10);
});

// The pixel theme keeps its own surfaces: nothing in the preview
// stylesheet may reach it.
it("leaves the pixel style untouched", () => {
  expect(matchingRules("lcd")).toEqual([]);
});
