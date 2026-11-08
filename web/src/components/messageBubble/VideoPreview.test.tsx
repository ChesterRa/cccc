// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { VideoPreview } from "./VideoPreview";
import { AttachmentResourceView } from "../resourcePanel/views/AttachmentResourceView";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string }) =>
      options?.name ? `${key}:${options.name}` : key,
  }),
}));

// Boundary: the browser's visibility signal; the component's reaction is real.
const observers: Array<{ callback: IntersectionObserverCallback; targets: Element[] }> = [];
class FakeIntersectionObserver {
  private readonly entry: { callback: IntersectionObserverCallback; targets: Element[] };
  constructor(callback: IntersectionObserverCallback) {
    this.entry = { callback, targets: [] };
    observers.push(this.entry);
  }
  observe(target: Element) {
    this.entry.targets.push(target);
  }
  disconnect() {
    this.entry.targets = [];
  }
}

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  observers.length = 0;
  vi.stubGlobal("IntersectionObserver", FakeIntersectionObserver);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  useResourcePanelStore.setState({ groups: {} });
});

const preview = (href: string, label = "clip.mp4") => (
  <VideoPreview
    key={label}
    resource={{
      panelGroupId: "g",
      blobGroupId: "g",
      messageKey: label,
      eventId: label,
      index: 0,
      blobPath: `state/blobs/${label}`,
    }}
    href={href}
    downloadHref={`${href}?download=true`}
    downloadName={label}
    label={label}
    downloadTitle={`Download ${label}`}
  />
);
const thumbnail = (label = "clip.mp4") =>
  host.querySelector<HTMLButtonElement>(`button[aria-label="playVideo:${label}"]`)!;
const reveal = (label = "clip.mp4") =>
  act(() => {
    const target = thumbnail(label);
    for (const observer of observers) {
      if (!observer.targets.includes(target)) continue;
      observer.callback(
        [{ isIntersecting: true, target } as unknown as IntersectionObserverEntry],
        observer as unknown as IntersectionObserver,
      );
    }
  });
const open = (label = "clip.mp4") => act(() => thumbnail(label).click());
const player = () => document.querySelector<HTMLVideoElement>("video[data-message-media]");

it("shows the first frame only once on screen and never plays it inline", () => {
  act(() => root.render(preview("/preview/clip.mp4")));
  const frame = thumbnail().querySelector("video")!;
  expect(frame.hasAttribute("src")).toBe(false);
  expect(frame.getAttribute("preload")).toBe("none");

  reveal();

  expect(frame.getAttribute("src")).toBe("/preview/clip.mp4#t=0.1");
  expect(frame.getAttribute("preload")).toBe("metadata");
  expect(frame.controls).toBe(false);
  expect(frame.muted).toBe(true);
  expect(player()).toBeNull();
});

it("opens a resource tab from the thumbnail", () => {
  act(() => root.render(preview("/preview/clip.mp4")));

  open();

  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(useResourcePanelStore.getState().groups.g.tabs[0]).toMatchObject({
    kind: "attachment",
    media: "video",
    eventId: "clip.mp4",
    title: "clip.mp4",
  });
});

it("pauses another playing video when one starts", () => {
  act(() =>
    root.render(
      <>
        <AttachmentResourceView
          media="video"
          href="/preview/a.mp4"
          title="a.mp4"
          isSvg={false}
          isDark={false}
          active
          resourceKey="a"
        />
        <AttachmentResourceView
          media="video"
          href="/preview/b.mp4"
          title="b.mp4"
          isSvg={false}
          isDark={false}
          active
          resourceKey="b"
        />
      </>,
    ),
  );
  const [first, second] = [
    ...document.querySelectorAll<HTMLVideoElement>("video[data-message-media]"),
  ];
  Object.defineProperty(first, "paused", { configurable: true, get: () => false });
  const pauseFirst = vi.spyOn(first, "pause").mockImplementation(() => undefined);
  const pauseSecond = vi.spyOn(second, "pause").mockImplementation(() => undefined);

  act(() => {
    second.dispatchEvent(new Event("play"));
  });

  expect(pauseFirst).toHaveBeenCalledTimes(1);
  expect(pauseSecond).not.toHaveBeenCalled();
});

it("falls back to the download link when the browser cannot play the video", () => {
  act(() => root.render(preview("/preview/clip.mp4")));
  reveal();

  act(() => {
    thumbnail().querySelector("video")!.dispatchEvent(new Event("error"));
  });

  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(host.querySelector("video")).toBeNull();
  expect(host.querySelector("[data-video-fallback]")?.textContent).toContain(
    "videoPlaybackUnavailable",
  );
  const link = host.querySelector<HTMLAnchorElement>('a[download="clip.mp4"]')!;
  expect(link.getAttribute("href")).toBe("/preview/clip.mp4?download=true");
  expect(link.textContent).toContain("clip.mp4");
});

it("gives a replacement video URL its own chance to play", () => {
  // A pending upload previews from a local URL that is revoked once stored.
  act(() => root.render(preview("blob:local-preview")));
  reveal();
  act(() => {
    thumbnail().querySelector("video")!.dispatchEvent(new Event("error"));
  });
  expect(host.querySelector("[data-video-fallback]")).not.toBeNull();

  act(() => root.render(preview("/api/v1/groups/g1/blobs/stored-hash?filename=clip.mp4")));
  expect(thumbnail().querySelector("video")?.getAttribute("src")).toBe(
    "/api/v1/groups/g1/blobs/stored-hash?filename=clip.mp4#t=0.1",
  );
});
