// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { readFileSync } from "node:fs";
import { fileURLToPath, URL as NodeURL } from "node:url";
import { ChetComposer } from "./ChetComposer";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

// The preview stylesheets, in the order preview.css imports them (happy-dom does not
// follow @import).
const read = (name: string) =>
  readFileSync(fileURLToPath(new NodeURL(`../../styles/${name}`, import.meta.url)), "utf8");
const sheetsCss = [...read("preview.css").matchAll(/@import\s+"\.\/([^"]+)"/g)].map((m) =>
  read(m[1]!),
);

const page = document.documentElement;
let host: HTMLDivElement;
let root: Root;
const sheets: HTMLStyleElement[] = [];

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const css of sheetsCss) {
    const sheet = document.createElement("style");
    sheet.textContent = css;
    document.head.append(sheet);
    sheets.push(sheet);
  }
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  sheets.splice(0).forEach((sheet) => sheet.remove());
  page.removeAttribute("data-ui-style");
  page.classList.remove("dark");
});

function render(style: string, draft: string) {
  page.setAttribute("data-ui-style", style);
  page.classList.add("dark");
  act(() =>
    root.render(
      <ChetComposer
        draft={draft}
        storageAvailable
        problem={null}
        duplicateRisk={false}
        onText={() => {}}
        onSend={() => {}}
        onDismissProblem={() => {}}
        onReviewRecovery={() => {}}
        voiceStatus={<select data-probe="voice" />}
      />,
    ),
  );
  const box = host.querySelector<HTMLElement>("[data-chet-composer-box]")!;
  return {
    box: getComputedStyle(box),
    field: getComputedStyle(box.querySelector("textarea")!),
    voice: getComputedStyle(box.querySelector('[data-probe="voice"]')!),
    send: getComputedStyle(box.querySelector('button[type="submit"]')!),
  };
}

it("draws the Chet composer as one box in preview, with quiet controls inside it", () => {
  const { box, field, voice, send } = render("preview", "");
  // The box carries the surface; the field inside it has none of its own.
  expect(box.borderRadius).toBe("12px");
  expect(box.backgroundColor).toBe("#1b1d24");
  expect(field.backgroundColor).toBe("transparent");
  expect(field.borderTopWidth).toBe("0px");
  expect(voice.backgroundColor).toBe("transparent");
  // Nothing to send: the key recedes instead of showing a dimmed solid block.
  expect(send.backgroundColor).toBe("transparent");
  expect(send.opacity).toBe("1");
});

it("leaves an enabled Send to the shared primary button, without the receded override", () => {
  // Tailwind's own classes are not loaded here, so "" means preview set no background.
  expect(render("preview", "hello").send.backgroundColor).toBe("");
});

it("leaves the composer to the pixel style's own rules", () => {
  const { box, send } = render("lcd", "");
  expect(box.borderRadius).toBe("");
  expect(send.backgroundColor).toBe("");
});
