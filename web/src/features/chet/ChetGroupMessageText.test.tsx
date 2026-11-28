// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { setUiStylePrefs } from "../../utils/uiStyle";
import { ChetGroupMessageText } from "./ChetGroupMessageText";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement;
let root: Root;
const REPORT =
  "Withdrew 4 questions; 114 need your login.\n\n- receipt 963913a8\n- receipt 8b2813d6";

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "lcd" });
});

const render = (text: string) => act(() => root.render(<ChetGroupMessageText text={text} />));
const toggle = () => host.querySelector<HTMLButtonElement>("button[aria-expanded]");

it("folds a long Group message to its first line in preview, and unfolds on request", () => {
  act(() => setUiStylePrefs({ style: "preview" }));
  render(REPORT);
  expect(host.querySelector("[data-chet-group-text]")?.getAttribute("data-chet-group-text")).toBe(
    "folded",
  );
  // Only the conclusion line shows; the receipts wait behind the toggle.
  expect(host.textContent).toContain("Withdrew 4 questions; 114 need your login.");
  expect(host.textContent).not.toContain("receipt 963913a8");
  expect(toggle()?.textContent).toBe("chet.groupMessageUnfold");

  act(() => toggle()!.click());
  expect(host.textContent).toContain("receipt 8b2813d6");
  expect(toggle()?.getAttribute("aria-expanded")).toBe("true");

  act(() => toggle()!.click());
  expect(host.textContent).not.toContain("receipt 963913a8");
});

it("shows a short message whole, with nothing to unfold", () => {
  act(() => setUiStylePrefs({ style: "preview" }));
  render("ok, done");
  expect(host.textContent).toBe("ok, done");
  expect(toggle()).toBeNull();
});

it("shows every message whole in the pixel style", () => {
  render(REPORT);
  expect(host.textContent).toBe(REPORT);
  expect(toggle()).toBeNull();
});
