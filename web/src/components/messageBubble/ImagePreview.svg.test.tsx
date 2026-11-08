// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ImagePreview } from "./ImagePreview";
import { AttachmentResourceView } from "../resourcePanel/views/AttachmentResourceView";

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
  ["image/png", 100],
  ["image/svg+xml", 460],
])("uses the fetched %s type rather than the .svg name when fitting", async (type, expected) => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({ ok: true, blob: async () => new Blob(["fixture"], { type }) }),
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
  await act(async () => image.dispatchEvent(new Event("load")));
  expect(Number.parseFloat(image.parentElement!.style.width)).toBe(expected);
});

it("does not allocate an object URL when an SVG response arrives after the source changes", async () => {
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
    finish(new Blob(["fixture"], { type: "image/svg+xml" }));
    await pending;
  });
  expect(URL.createObjectURL).not.toHaveBeenCalled();
});
