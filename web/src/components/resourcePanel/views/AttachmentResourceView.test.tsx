// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { AttachmentResourceView, type AttachmentResourceMedia } from "./AttachmentResourceView";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
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

function preview(media: AttachmentResourceMedia, href: string, active = true, isSvg = false) {
  return (
    <AttachmentResourceView
      media={media}
      href={href}
      title="fixture"
      isSvg={isSvg}
      isDark={false}
      active={active}
      resourceKey="attachment:fixture"
    />
  );
}

it("owns the SVG image URL until its resource view closes", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue({
        ok: true,
        blob: async () => new Blob(['<svg><script id="bad" /></svg>'], { type: "image/svg+xml" }),
      }),
  );
  const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:resource-owned");
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
  await act(async () => root.render(preview("image", "/fixture.svg", true, true)));
  expect(create).toHaveBeenCalledOnce();
  expect(
    host.querySelector<HTMLImageElement>("[data-graphic-image] img")?.getAttribute("src"),
  ).toBe("blob:resource-owned");
  expect(host.querySelector("script#bad")).toBeNull();
  await act(async () => root.render(null));
  expect(revoke).toHaveBeenCalledWith("blob:resource-owned");
});

it("does not allocate an SVG URL after the resource changes while its response is pending", async () => {
  let finish!: (blob: Blob) => void;
  const pending = new Promise<Blob>((resolve) => {
    finish = resolve;
  });
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, blob: () => pending }));
  const create = vi.spyOn(URL, "createObjectURL");
  await act(async () => root.render(preview("image", "/slow.svg", true, true)));
  await act(async () => root.render(preview("image", "/other.png")));
  await act(async () => {
    finish(new Blob(["fixture"], { type: "image/svg+xml" }));
    await pending;
  });
  expect(create).not.toHaveBeenCalled();
  expect(
    host.querySelector<HTMLImageElement>("[data-graphic-image] img")?.getAttribute("src"),
  ).toBe("/other.png");
});

it("pauses a hidden video without replacing the player or losing its position", async () => {
  await act(async () => root.render(preview("video", "/clip.mp4")));
  const video = host.querySelector("video")!;
  Object.defineProperty(video, "currentTime", { configurable: true, writable: true, value: 23 });
  const pause = vi.spyOn(video, "pause").mockImplementation(() => undefined);
  await act(async () => root.render(preview("video", "/clip.mp4", false)));
  expect(pause).toHaveBeenCalledOnce();
  expect(host.querySelector("video")).toBe(video);
  expect(video.currentTime).toBe(23);
  await act(async () => root.render(preview("video", "/clip.mp4")));
  expect(host.querySelector("video")).toBe(video);
  expect(video.currentTime).toBe(23);
});

it("keeps the same PDF frame when the selected resource becomes inactive", async () => {
  const fixture = "data:application/pdf;base64,JVBERi0xLjcK";
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(
        new Response("%PDF-1.7\n", { headers: { "content-type": "application/pdf" } }),
      ),
  );
  await act(async () => root.render(preview("pdf", fixture)));
  const frame = host.querySelector("iframe");
  expect(frame?.getAttribute("src")).toBe(fixture);
  await act(async () => root.render(preview("pdf", fixture, false)));
  await act(async () => root.render(preview("pdf", fixture)));
  expect(host.querySelector("iframe")).toBe(frame);
});
