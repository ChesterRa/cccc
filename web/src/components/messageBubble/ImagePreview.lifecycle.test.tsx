// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ImagePreview } from "./ImagePreview";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.spyOn(HTMLImageElement.prototype, "decode").mockImplementation(() => new Promise(() => {}));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("releases the modal lock on thumbnail failure and does not reopen after recovery", async () => {
  const render = async (href: string) => {
    await act(async () =>
      root.render(
        <ImagePreview
          href={href}
          downloadHref={href}
          downloadName="image.png"
          alt="Image"
          isSvg={false}
          isUserMessage={false}
          isDark={false}
        />,
      ),
    );
  };
  const original = {
    position: document.body.style.position,
    overflow: document.body.style.overflow,
  };
  await render("/slow-lifecycle.png");
  const thumbnail = host.querySelector("img")!;
  await act(async () => host.querySelector("button")!.click());
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(document.body.style.position).toBe("fixed");
  await act(async () => thumbnail.dispatchEvent(new Event("error")));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.body.style.position).toBe(original.position);
  expect(document.body.style.overflow).toBe(original.overflow);
  expect(host.textContent).toContain("imagePreviewUnavailable");
  await render("/recovered-lifecycle.png");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // The previous failed preview must not reject the replacement being preloaded.
  expect(host.querySelector("img")!.getAttribute("src")).toBe("/slow-lifecycle.png");
  await act(async () => host.querySelector("img")!.dispatchEvent(new Event("error")));
  expect(host.textContent).not.toContain("imagePreviewUnavailable");
  const opener = host.querySelector<HTMLButtonElement>("button")!;
  opener.focus();
  await act(async () => opener.click());
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
  );
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(opener);
  expect(document.body.style.position).toBe(original.position);
});
