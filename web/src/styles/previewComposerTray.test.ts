// @vitest-environment happy-dom
import { afterEach, expect, it } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";

const css = readFileSync(
  fileURLToPath(new NodeURL("./previewComposerTray.css", import.meta.url)),
  "utf8",
);
const root = document.documentElement;
let sheet: HTMLStyleElement | null = null;

afterEach(() => {
  sheet?.remove();
  document.body.innerHTML = "";
  root.removeAttribute("data-ui-style");
  root.removeAttribute("data-lcd-motion");
});

function render(style: string, motion: "on" | "off" = "on") {
  root.setAttribute("data-ui-style", style);
  root.setAttribute("data-lcd-motion", motion);
  sheet = document.createElement("style");
  sheet.textContent = css;
  document.head.append(sheet);
  document.body.innerHTML = `
    <div data-composer-surface>
      <ul data-composer-file-tray><li><div data-composer-file-preview></div></li></ul>
    </div>`;
  return {
    tray: getComputedStyle(document.querySelector("[data-composer-file-tray]")!),
    file: getComputedStyle(document.querySelector("[data-composer-file-tray] > li")!),
  };
}

it("floats waiting files over the conversation in the preview style", () => {
  const { tray, file } = render("preview");
  expect(tray.position).toBe("absolute");
  // The gaps between files let clicks through; the files themselves take them.
  expect(tray.pointerEvents).toBe("none");
  expect(file.pointerEvents).toBe("auto");
  expect(file.animation).toContain("preview-file-drift");
});

it("keeps waiting files inside the composer in the pixel style", () => {
  const { tray, file } = render("lcd");
  expect(tray.position).not.toBe("absolute");
  expect(file.animation).not.toContain("preview-file-drift");
});

it("holds floating files still when motion is turned off", () => {
  const { tray, file } = render("preview", "off");
  expect(tray.position).toBe("absolute");
  expect(file.animation).toBe("none");
});
