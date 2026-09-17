// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useTerminalTitlePaging } from "./useTerminalTitlePaging";

let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const page = vi.fn();
function Fixture({ enabled = true }) {
  const gestures = useTerminalTitlePaging(enabled ? page : undefined);
  return (
    <>
      <div data-header {...gestures}>
        <span data-title>Actor</span>
        <button>Actions</button>
      </div>
      <div data-body>Terminal</div>
    </>
  );
}
function pointer(
  type: string,
  x: number,
  y = 20,
  target = "[data-title]",
  options: PointerEventInit = {},
) {
  host
    .querySelector(target)!
    .dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        pointerId: 1,
        pointerType: "touch",
        isPrimary: true,
        clientX: x,
        clientY: y,
        ...options,
      }),
    );
}
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  page.mockClear();
  await act(async () => root.render(<Fixture />));
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("pages left and right only after a deliberate title-bar swipe", () => {
  pointer("pointerdown", 180);
  pointer("pointerup", 100);
  pointer("pointerdown", 100);
  pointer("pointerup", 180);
  expect(page.mock.calls).toEqual([[1], [-1]]);
});
it.each(["button", "[data-body]"])("leaves %s gestures alone", (target) => {
  pointer("pointerdown", 180, 20, target);
  pointer("pointerup", 100, 20, target);
  expect(page).not.toHaveBeenCalled();
});
it("ignores taps, mouse drags, vertical scrolling, canceled and multiple touches", () => {
  pointer("pointerdown", 180);
  pointer("pointerup", 160);
  pointer("pointerdown", 180, 20, "[data-title]", { pointerType: "mouse" });
  pointer("pointerup", 100);
  pointer("pointerdown", 180);
  pointer("pointermove", 170, 50);
  pointer("pointerup", 100);
  pointer("pointerdown", 180);
  pointer("pointercancel", 180);
  pointer("pointerup", 100);
  pointer("pointerdown", 180);
  pointer("pointerdown", 170, 20, "[data-title]", { pointerId: 2, isPrimary: false });
  pointer("pointerup", 100);
  expect(page).not.toHaveBeenCalled();
});
it("does not finish a gesture after the retained view becomes hidden or expanded", async () => {
  pointer("pointerdown", 180);
  await act(async () => root.render(<Fixture enabled={false} />));
  pointer("pointerup", 100);
  expect(page).not.toHaveBeenCalled();
});
