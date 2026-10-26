// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { VISUAL_VIEWPORT_BOX } from "../../hooks/useViewportHeight";
import { ModalFrame } from "./ModalFrame";
import { getModalOverlayStyle } from "./modalOverlayStyle";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(inline = false) {
  act(() =>
    root.render(
      <ModalFrame
        isOpen
        inline={inline}
        isDark
        onClose={() => {}}
        titleId="t"
        title=""
        closeAriaLabel="close"
        panelClassName=""
      >
        <textarea aria-label="terminal input" />
      </ModalFrame>,
    ),
  );
  return host.firstElementChild as HTMLElement;
}

it("sizes a full-screen modal from the visible viewport so the mobile keyboard cannot cover it", () => {
  // useViewportHeight publishes the visible viewport (shrunk and panned by the iOS
  // keyboard) as CSS variables; a fixed overlay must size against them rather than
  // the layout viewport, or a terminal's input row ends up under the keyboard.
  for (const isOpen of [true, false]) {
    expect(getModalOverlayStyle({ inline: false, isOpen })).toMatchObject({
      top: "var(--app-viewport-offset-top, 0px)",
      height: "var(--app-viewport-height, 100dvh)",
      maxHeight: "var(--app-viewport-height, 100dvh)",
      bottom: "auto",
    });
  }
  // The rendered overlay uses that style (happy-dom keeps max-height but drops
  // var() values for height/top, so only max-height is observable here).
  expect(render().style.getPropertyValue("max-height")).toBe(VISUAL_VIEWPORT_BOX.maxHeight);
});

it("leaves inline frames to their host layout", () => {
  expect(getModalOverlayStyle({ inline: true, isOpen: true })).toBeUndefined();
  expect(render(true).getAttribute("style")).toBeNull();
});

it.each(["", "Native editor"])(
  "preserves default close behavior and visibly blocks close while disabled (title=%s)",
  (title) => {
    const onClose = vi.fn();
    const show = (closeDisabled?: boolean) =>
      act(() =>
        root.render(
          <ModalFrame
            isDark={false}
            onClose={onClose}
            titleId="native-editor"
            title={title}
            closeAriaLabel="Close"
            closeDisabled={closeDisabled}
            panelClassName=""
          >
            <p>Editable metadata</p>
          </ModalFrame>,
        ),
      );
    show();
    const close = () => host.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!;
    const overlay = () => host.querySelector(".glass-overlay")!;
    expect(close().disabled).toBe(false);
    act(() => close().click());
    act(() => overlay().dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(onClose).toHaveBeenCalledTimes(2);
    show(true);
    expect(close().disabled).toBe(true);
    expect(close().className).toContain("disabled:opacity-50");
    act(() => close().click());
    act(() => overlay().dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(onClose).toHaveBeenCalledTimes(2);
    show(false);
    act(() => close().click());
    expect(onClose).toHaveBeenCalledTimes(3);
  },
);
