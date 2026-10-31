// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { useGroupMenu } from "./useGroupMenu";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function Harness() {
  const menu = useGroupMenu("Group actions", [{ label: "Pause", onClick: () => undefined }]);
  return (
    <>
      <div data-lcd-scope>
        <button type="button" data-trigger="inside" onClick={(e) => menu.toggle(e.currentTarget)}>
          inside
        </button>
        <div data-row="inside" onContextMenu={menu.onContextMenu}>
          <button type="button">row</button>
        </div>
        <button type="button" data-key-row="inside" onKeyDown={menu.onKeyDown}>
          keyboard row
        </button>
      </div>
      <button type="button" data-trigger="outside" onClick={(e) => menu.toggle(e.currentTarget)}>
        outside
      </button>
      {menu.menu}
    </>
  );
}

let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<Harness />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const menu = () => document.querySelector<HTMLElement>("[data-group-menu]");

// The menu renders in a portal, outside the scope its trigger sits in.
it("gives the menu the LCD scope of a control inside the scope", async () => {
  await act(async () => host.querySelector<HTMLButtonElement>('[data-trigger="inside"]')!.click());
  expect(menu()?.hasAttribute("data-lcd-scope")).toBe(true);
});

it("leaves the menu unscoped for a control outside the scope", async () => {
  await act(async () => host.querySelector<HTMLButtonElement>('[data-trigger="outside"]')!.click());
  expect(menu()).not.toBeNull();
  expect(menu()?.hasAttribute("data-lcd-scope")).toBe(false);
});

it("takes the scope of a row opened with the context menu", async () => {
  const row = host.querySelector<HTMLElement>('[data-row="inside"]')!;
  await act(async () => {
    row.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 8, clientY: 8 }),
    );
  });
  expect(menu()?.hasAttribute("data-lcd-scope")).toBe(true);
});

it("takes the scope of a row opened from the keyboard", async () => {
  const row = host.querySelector<HTMLButtonElement>('[data-key-row="inside"]')!;
  await act(async () => {
    row.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true }));
  });
  expect(menu()?.hasAttribute("data-lcd-scope")).toBe(true);
});
