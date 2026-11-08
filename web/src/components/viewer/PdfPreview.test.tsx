// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { PdfPreview } from "./PdfPreview";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./PdfCanvasViewer", () => ({
  PdfCanvasViewer: ({ href }: { href: string }) => <div data-pdf-canvas-viewer={href} />,
}));

let root: Root;
let host: HTMLDivElement;
let coarse: boolean;
let pdfViewerEnabled: boolean;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  coarse = false;
  pdfViewerEnabled = true;
  vi.stubGlobal("matchMedia", () => ({
    matches: coarse,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  Object.defineProperty(navigator, "pdfViewerEnabled", {
    configurable: true,
    get: () => pdfViewerEnabled,
  });
  host = document.createElement("div");
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

async function render() {
  await act(async () => {
    root.render(
      <PdfPreview href="/fixture.pdf" title="Report" hint="Browser" failedText="Failed" />,
    );
  });
}

it("keeps the browser's PDF viewer for a desktop pointer", async () => {
  await render();
  expect(host.querySelector("iframe")?.getAttribute("src")).toBe("/fixture.pdf");
  expect(host.querySelector("[data-pdf-canvas-viewer]")).toBeNull();
});

it("does not add a hint when the surrounding viewer already has its own controls", async () => {
  await act(async () => {
    root.render(<PdfPreview href="/fixture.pdf" title="Report" failedText="Failed" />);
  });
  expect(host.querySelector("iframe")).not.toBeNull();
  expect(host.querySelector("p")).toBeNull();
});

it("uses the in-page renderer on touch even when the browser reports PDF support", async () => {
  coarse = true;
  await render();
  expect(host.querySelector("iframe")).toBeNull();
  expect(
    host.querySelector("[data-pdf-canvas-viewer]")?.getAttribute("data-pdf-canvas-viewer"),
  ).toBe("/fixture.pdf");
});

it("uses the in-page renderer when a desktop browser disables its PDF viewer", async () => {
  pdfViewerEnabled = false;
  await render();
  expect(host.querySelector("iframe")).toBeNull();
  expect(host.querySelector("[data-pdf-canvas-viewer]")).not.toBeNull();
});
