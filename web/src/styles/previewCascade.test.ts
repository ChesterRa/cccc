// @vitest-environment happy-dom
import { afterEach, expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";

// preview.css is the entry the app loads for the preview style; happy-dom
// does not follow @import, so its imports are loaded here in the order it lists them.
const read = (name: string) =>
  readFileSync(fileURLToPath(new NodeURL(`./${name}`, import.meta.url)), "utf8");
const imports = [...read("preview.css").matchAll(/@import\s+"\.\/([^"]+)"/g)].map((m) => m[1]!);

const root = document.documentElement;
const sheets: HTMLStyleElement[] = [];

afterEach(() => {
  sheets.splice(0).forEach((sheet) => sheet.remove());
  document.body.innerHTML = "";
  root.removeAttribute("data-ui-style");
  root.classList.remove("dark");
});

function render(style: string, html: string, theme: "dark" | "light" = "dark") {
  root.setAttribute("data-ui-style", style);
  root.classList.toggle("dark", theme === "dark");
  for (const name of imports) {
    const sheet = document.createElement("style");
    sheet.textContent = read(name);
    document.head.append(sheet);
    sheets.push(sheet);
  }
  document.body.innerHTML = html;
}

const MESSAGES = `
  <div class="lcd-row" data-probe="agent"><div class="lcd-row-content" data-probe="agent-body"></div></div>
  <div class="lcd-row" data-lcd-user><div class="lcd-row-content" data-probe="user-body"></div></div>
`;
const style = (probe: string) =>
  getComputedStyle(document.querySelector(`[data-probe="${probe}"]`)!);

it("lets the preview theme refine the layout rules it ties with", () => {
  render("preview", MESSAGES);
  // Tighter rows than the layout file's 0.5rem.
  expect(style("agent").getPropertyValue("padding-block")).toBe("0.25rem");
  // The preview corner scale, keeping the small corner toward the sender.
  expect(style("agent-body").borderTopLeftRadius).toBe("4px");
  expect(style("agent-body").borderTopRightRadius).toBe("12px");
  expect(style("user-body").borderTopRightRadius).toBe("4px");
  expect(style("user-body").borderTopLeftRadius).toBe("12px");
});

it("applies none of the preview layout or theme in the pixel style", () => {
  render("lcd", MESSAGES);
  expect(style("agent").getPropertyValue("padding-block")).toBe("");
  expect(style("agent-body").borderTopLeftRadius).toBe("");
});

it("keeps dialog inputs above the dialog surface and body text untracked in preview", () => {
  render(
    "preview",
    `<div class="glass-modal"><input class="glass-input" data-probe="input" /></div>`,
  );
  expect(style("input").backgroundColor).toBe("#1b1d24");
  expect(getComputedStyle(root).letterSpacing).not.toMatch(/^-/);
});

it("paints light preview from its own palette: white messages, inset fields, ink text", () => {
  render(
    "preview",
    `${MESSAGES}<div class="glass-modal" data-probe="modal"><input class="glass-input" data-probe="input" /></div>`,
    "light",
  );
  expect(style("agent-body").backgroundColor).toBe("#ffffff");
  expect(style("user-body").backgroundColor).toBe("#e9e9e4");
  expect(style("modal").backgroundColor).toBe("#ffffff");
  expect(style("input").backgroundColor).toBe("#f6f6f3");
});

it("keeps the dark palette for the same surfaces", () => {
  render("preview", MESSAGES);
  expect(style("agent-body").backgroundColor).toBe("#1a1c23");
  expect(style("user-body").backgroundColor).toBe("#232732");
});
