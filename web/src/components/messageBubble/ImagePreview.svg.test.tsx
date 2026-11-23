// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ImagePreview } from "./ImagePreview";
import { AttachmentResourceView } from "../resourcePanel/views/AttachmentResourceView";
import { MAX_SVG_PREVIEW_BYTES, resolveSvgImageSource } from "./svgImageSource";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLImageElement.prototype, "naturalWidth", "get").mockReturnValue(100);
  vi.spyOn(HTMLImageElement.prototype, "naturalHeight", "get").mockReturnValue(80);
  vi.spyOn(HTMLImageElement.prototype, "decode").mockImplementation(() => new Promise(() => {}));
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:fixture-image");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private callback: ResizeObserverCallback) {}
      observe() {
        this.callback(
          [{ contentRect: { width: 500, height: 400 } } as ResizeObserverEntry],
          this as unknown as ResizeObserver,
        );
      }
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

it.each([
  ["image/png", "fixture", 100, "blob:fixture-image"],
  ["image/svg+xml", "<svg xmlns='http://www.w3.org/2000/svg'/>", 460, "data:image/svg+xml;base64,"],
  [
    "application/octet-stream",
    '<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"/>',
    460,
    "data:image/svg+xml;base64,",
  ],
])(
  "uses the fetched %s payload safely when fitting and avoids same-origin blob SVG URLs",
  async (type, body, expected, expectedSrcPrefix) => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, blob: async () => new Blob([body], { type }) }),
    );
    await act(async () => {
      root.render(
        <AttachmentResourceView
          media="image"
          href="/api/v1/groups/g/blobs/content?filename=picture.svg"
          title="picture.svg"
          isSvg
          isDark={false}
          active
          resourceKey="fixture"
        />,
      );
    });
    await vi.waitFor(() => expect(host.querySelector("[data-graphic-image] img")).not.toBeNull());
    const image = host.querySelector<HTMLImageElement>("[data-graphic-image] img")!;
    expect(image.getAttribute("src")?.startsWith(expectedSrcPrefix)).toBe(true);
    if (type !== "image/png") {
      expect(URL.createObjectURL).not.toHaveBeenCalled();
    }
    await act(async () => image.dispatchEvent(new Event("load")));
    expect(Number.parseFloat(image.parentElement!.style.width)).toBe(expected);
  },
);

it("renders octet-stream SVG inline in ImagePreview via opaque data URL and rejects oversized blobs", async () => {
  const validSvg = '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"></svg>';
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue({
        ok: true,
        blob: async () => new Blob([validSvg], { type: "application/octet-stream" }),
      }),
  );
  await act(async () => {
    root.render(
      <ImagePreview
        resource={{
          panelGroupId: "g",
          blobGroupId: "g",
          messageKey: "event",
          eventId: "event",
          index: 0,
          blobPath: "state/blobs/icon",
          siblings: [],
        }}
        href="/api/v1/groups/g/blobs/icon?filename=icon.svg"
        downloadHref="/api/v1/groups/g/blobs/icon?filename=icon.svg&download=1"
        downloadName="icon.svg"
        alt="icon.svg"
        isSvg
        isUserMessage={false}
        isDark={false}
      />,
    );
  });
  await vi.waitFor(() => expect(host.querySelector("img")).not.toBeNull());
  const img = host.querySelector<HTMLImageElement>("img")!;
  expect(img.getAttribute("src")?.startsWith("data:image/svg+xml;base64,")).toBe(true);
  expect(URL.createObjectURL).not.toHaveBeenCalled();

  const oversized = new Blob([new Uint8Array(MAX_SVG_PREVIEW_BYTES + 1)], {
    type: "image/svg+xml",
  });
  await expect(resolveSvgImageSource(oversized)).rejects.toThrow("svg_too_large");
});

it("does not allocate an object URL when a response arrives after the source changes", async () => {
  let finish!: (blob: Blob) => void;
  const pending = new Promise<Blob>((resolve) => {
    finish = resolve;
  });
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, blob: () => pending }));
  const preview = (href: string, isSvg: boolean) => (
    <ImagePreview
      resource={{
        panelGroupId: "g",
        blobGroupId: "g",
        messageKey: "event",
        eventId: "event",
        index: 0,
        blobPath: "state/blobs/image",
        siblings: [],
      }}
      href={href}
      downloadHref={href}
      downloadName="picture.svg"
      alt="picture"
      isSvg={isSvg}
      isUserMessage={false}
      isDark={false}
    />
  );
  await act(async () => root.render(preview("/slow.svg", true)));
  await act(async () => root.render(preview("/next.png", false)));
  await act(async () => {
    finish(new Blob(["fixture"], { type: "image/png" }));
    await pending;
  });
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});
