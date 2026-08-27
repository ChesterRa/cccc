// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useForcedBottomFollowKeyboardCancel } from "./useForcedBottomFollow";

describe("useForcedBottomFollowKeyboardCancel", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("cancels forced follow when a focused descendant requests keyboard scrolling", async () => {
    const cancel = vi.fn();

    function Probe() {
      const onKeyDownCapture = useForcedBottomFollowKeyboardCancel(cancel);
      return (
        <div onKeyDownCapture={onKeyDownCapture}>
          <button type="button">message action</button>
        </div>
      );
    }

    await act(async () => root.render(<Probe />));
    const button = host.querySelector("button");
    button?.focus();
    button?.dispatchEvent(new KeyboardEvent("keydown", { key: "PageUp", bubbles: true }));

    expect(cancel).toHaveBeenCalledOnce();
  });

  it("does not cancel forced follow for non-scrolling keys", async () => {
    const cancel = vi.fn();

    function Probe() {
      return <div onKeyDownCapture={useForcedBottomFollowKeyboardCancel(cancel)} />;
    }

    await act(async () => root.render(<Probe />));
    host.firstElementChild?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    expect(cancel).not.toHaveBeenCalled();
  });
});
