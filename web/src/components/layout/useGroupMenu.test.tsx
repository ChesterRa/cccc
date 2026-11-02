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
      <button type="button" data-trigger onClick={(e) => menu.toggle(e.currentTarget)}>
        button
      </button>
      <div data-row onContextMenu={menu.onContextMenu}>
        <button type="button">row</button>
      </div>
      <button type="button" data-key-row onKeyDown={menu.onKeyDown}>
        keyboard row
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

it("opens from its button", async () => {
  await act(async () => host.querySelector<HTMLButtonElement>("[data-trigger]")!.click());
  expect(menu()?.textContent).toContain("Pause");
});

it("opens from a row's context menu", async () => {
  const row = host.querySelector<HTMLElement>("[data-row]")!;
  await act(async () => {
    row.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 8, clientY: 8 }),
    );
  });
  expect(menu()?.textContent).toContain("Pause");
});

it("opens from the keyboard", async () => {
  const row = host.querySelector<HTMLButtonElement>("[data-key-row]")!;
  await act(async () => {
    row.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true }));
  });
  expect(menu()?.textContent).toContain("Pause");
});
