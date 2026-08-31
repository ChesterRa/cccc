// @vitest-environment happy-dom

import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useComposerTextareaAutoResize } from "./useComposerTextareaAutoResize";

function Probe({
  value,
  minHeight = 52,
  maxHeight = 128,
}: {
  value: string;
  minHeight?: number;
  maxHeight?: number;
}) {
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  useComposerTextareaAutoResize({ composerRef, value, minHeight, maxHeight });
  return <textarea ref={composerRef} value={value} readOnly />;
}

describe("useComposerTextareaAutoResize", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let scrollHeightDescriptor: PropertyDescriptor | undefined;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    scrollHeightDescriptor = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "scrollHeight",
    );
    Object.defineProperty(HTMLTextAreaElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        const lines = Math.max(1, (this as HTMLTextAreaElement).value.split("\n").length);
        return lines * 44 + 8;
      },
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    if (scrollHeightDescriptor) {
      Object.defineProperty(HTMLTextAreaElement.prototype, "scrollHeight", scrollHeightDescriptor);
    } else {
      delete (HTMLTextAreaElement.prototype as { scrollHeight?: number }).scrollHeight;
    }
  });

  it("converges to the current committed value before paint without queued frame writes", async () => {
    const requestFrame = vi.spyOn(window, "requestAnimationFrame");

    await act(async () => root.render(<Probe value="one line" />));
    expect(host.querySelector("textarea")?.style.height).toBe("52px");

    await act(async () => root.render(<Probe value={"first\nsecond"} />));
    expect(host.querySelector("textarea")?.style.height).toBe("96px");

    await act(async () => root.render(<Probe value="short again" />));
    expect(host.querySelector("textarea")?.style.height).toBe("52px");
    expect(requestFrame).not.toHaveBeenCalled();
  });

  it("caps long drafts at the composer maximum", async () => {
    await act(async () => root.render(<Probe value={"1\n2\n3\n4"} />));
    expect(host.querySelector("textarea")?.style.height).toBe("128px");
  });

  it("keeps a fixed desktop editing viewport while the draft grows", async () => {
    await act(async () => root.render(<Probe value="one line" minHeight={64} maxHeight={64} />));
    expect(host.querySelector("textarea")?.style.height).toBe("64px");

    await act(async () =>
      root.render(<Probe value={"1\n2\n3\n4"} minHeight={64} maxHeight={64} />),
    );
    expect(host.querySelector("textarea")?.style.height).toBe("64px");
  });
});
