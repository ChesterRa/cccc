// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

import { ModalFrame } from "./ModalFrame";

describe("ModalFrame footer", () => {
  it("keeps its base bottom padding and adds the device safe area on top", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () =>
      root.render(
        <ModalFrame
          isDark={false}
          onClose={vi.fn()}
          titleId="frame-title"
          title="Frame"
          closeAriaLabel="Close"
          panelClassName="w-full"
          footerActions={<button type="button">Save</button>}
        >
          <div>body</div>
        </ModalFrame>,
      ),
    );

    const footer = host.querySelector("button[type=button]")!.parentElement!;
    expect(footer.className).toContain("pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))]");
    expect(footer.className).toContain("sm:pb-[calc(1rem+env(safe-area-inset-bottom,0px))]");
    // The old unlayered helper class outranked every padding utility and
    // zeroed the footer's bottom padding on desktop.
    expect(footer.className).not.toContain("safe-area-inset-bottom ");

    await act(async () => root.unmount());
    host.remove();
  });
});
