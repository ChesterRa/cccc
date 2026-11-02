// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { InfoPopover } from "./InfoPopover";

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
      <InfoPopover isDark={false} title="Access" content="Who can sign in">
        {(getReferenceProps, setReference) => (
          <button type="button" ref={setReference as never} {...getReferenceProps()}>
            Help
          </button>
        )}
      </InfoPopover>,
    );
  });
}

it("opens with its title and closes from its trigger", async () => {
  await render();
  const trigger = host.querySelector<HTMLButtonElement>("button")!;
  await act(async () => trigger.click());
  const popup = document.querySelector<HTMLElement>("[data-settings-info-popover]")!;
  expect(popup.textContent).toContain("Who can sign in");
  expect(document.getElementById(popup.getAttribute("aria-labelledby")!)?.textContent).toBe(
    "Access",
  );
  await act(async () => trigger.click());
  expect(document.querySelector("[data-settings-info-popover]")).toBeNull();
});
