// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { ScopeTooltip } from "./ScopeTooltip";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function render() {
  await act(async () => {
    root.render(
      <div>
        <button type="button" disabled>
          Instance
        </button>
        <ScopeTooltip title="Settings scope" content="Admin access required" />
      </div>,
    );
  });
}

it("opens independently of a disabled scope", async () => {
  await render();
  const trigger = host.querySelector<HTMLButtonElement>("[data-scope-help-trigger]")!;
  expect(trigger.disabled).toBe(false);
  await act(async () => trigger.click());
  const popup = document.querySelector<HTMLElement>("[data-scope-help-popup]")!;
  expect(popup.textContent).toContain("Admin access required");
  await act(async () => trigger.click());
  expect(document.querySelector("[data-scope-help-popup]")).toBeNull();
});
