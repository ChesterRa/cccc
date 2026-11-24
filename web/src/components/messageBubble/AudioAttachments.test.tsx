// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi, type MockInstance } from "vite-plus/test";
import type { MessageAttachment } from "../../types";
import { AudioAttachments } from "./AudioAttachments";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { name?: string }) => (args?.name ? `${key} ${args.name}` : key),
  }),
}));

const clip = (name: string): MessageAttachment => ({
  kind: "file",
  path: `state/blobs/${name}-hash`,
  title: name,
  mime_type: "audio/mpeg",
});

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let play: MockInstance<() => Promise<void>>;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(names: string[]) {
  await act(async () =>
    root.render(
      <AudioAttachments
        attachments={names.map(clip)}
        blobGroupId="g1"
        attachmentKeyPrefix="event-1"
        downloadTitle={(name) => `Download ${name}`}
      />,
    ),
  );
}

const audio = () => host.querySelector<HTMLAudioElement>("audio[data-message-media]")!;
const button = (label: string) => host.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;

it("keeps each download and loads nothing until a file is chosen", async () => {
  await render(["voice-F1.mp3", "voice-F2.mp3"]);

  expect(host.querySelectorAll("audio")).toHaveLength(1);
  expect(audio().getAttribute("src")).toBeNull();
  expect(audio().getAttribute("preload")).toBe("none");
  // There is no visible player: the chips alone control playback.
  expect(audio().hidden).toBe(true);
  expect(audio().hasAttribute("controls")).toBe(false);
  const links = host.querySelectorAll("a[download]");
  expect([...links].map((link) => link.getAttribute("download"))).toEqual([
    "voice-F1.mp3",
    "voice-F2.mp3",
  ]);
  // A button inside a link would make every play click a download as well.
  expect(host.querySelector("a button")).toBeNull();
  expect(links[0].getAttribute("href")).toContain("download=true");
});

it("plays from the whole file chip and downloads from its own key", async () => {
  await render(["voice-F1.mp3"]);
  const toggle = host.querySelector<HTMLButtonElement>("[data-message-audio-toggle]")!;
  // The name is part of the play key, so clicking the file plays it.
  expect(toggle.textContent).toBe("voice-F1.mp3");
  expect(toggle.getAttribute("aria-label")).toBe("playAudio voice-F1.mp3");
  const download = host.querySelector<HTMLAnchorElement>("a[download]")!;
  expect(download.textContent).toBe("");
  expect(download.getAttribute("aria-label")).toBe("Download voice-F1.mp3");
  expect(toggle.contains(download)).toBe(false);
});

it("shows how far the playing clip has got along its own chip", async () => {
  await render(["voice-F1.mp3", "voice-F2.mp3"]);
  vi.spyOn(audio(), "play").mockResolvedValue(undefined);
  act(() => button("playAudio voice-F1.mp3").click());
  const bars = () => host.querySelectorAll<HTMLElement>("[data-message-audio-progress]");
  expect(bars()).toHaveLength(1);
  expect(bars()[0].closest("[data-message-audio]")?.textContent).toContain("voice-F1.mp3");
  Object.defineProperty(audio(), "duration", { value: 8, configurable: true });
  Object.defineProperty(audio(), "currentTime", { value: 2, configurable: true });
  act(() => audio().dispatchEvent(new Event("timeupdate")));
  expect(bars()[0].style.width).toBe("25%");
  // A stream with no known length shows no progress rather than a wrong one.
  Object.defineProperty(audio(), "duration", {
    value: Number.POSITIVE_INFINITY,
    configurable: true,
  });
  act(() => audio().dispatchEvent(new Event("timeupdate")));
  expect(bars()[0].style.width).toBe("0%");
  // Switching clips starts the new one's bar from the beginning.
  act(() => button("playAudio voice-F2.mp3").click());
  expect(bars()).toHaveLength(1);
  expect(bars()[0].closest("[data-message-audio]")?.textContent).toContain("voice-F2.mp3");
  expect(bars()[0].style.width).toBe("0%");
});

it("sets the source and starts playback inside the click", async () => {
  await render(["voice-F1.mp3"]);
  let sourceWhenStarted = "";
  play.mockImplementation(async () => {
    sourceWhenStarted = audio().getAttribute("src") || "";
  });

  // Not wrapped in an async act: a browser only honours play() during the gesture.
  act(() => button("playAudio voice-F1.mp3").click());

  expect(play).toHaveBeenCalledTimes(1);
  expect(sourceWhenStarted).toContain("voice-F1.mp3");
  expect(sourceWhenStarted).not.toContain("download=true");
  expect(audio().hidden).toBe(true);
  // The highlighted chip names the clip; the player names it for assistive technology.
  expect(audio().getAttribute("aria-label")).toBe("voice-F1.mp3");
});

it("switches to another file with one click and shows pause from the media state", async () => {
  await render(["voice-F1.mp3", "voice-F2.mp3"]);
  act(() => button("playAudio voice-F1.mp3").click());
  act(() => {
    audio().dispatchEvent(new Event("play"));
  });
  expect(button("pauseAudio voice-F1.mp3")).not.toBeNull();

  act(() => button("playAudio voice-F2.mp3").click());

  expect(audio().getAttribute("src")).toContain("voice-F2.mp3");
  expect(play).toHaveBeenCalledTimes(2);
  // The highlighted chip names the clip; the player names it for assistive technology.
  expect(audio().getAttribute("aria-label")).toBe("voice-F2.mp3");
  // Pause shows only once the media reports it started; a refused start stays on play.
  expect(button("playAudio voice-F2.mp3")).not.toBeNull();
  act(() => {
    audio().dispatchEvent(new Event("play"));
  });
  expect(button("pauseAudio voice-F2.mp3")).not.toBeNull();
  expect(button("playAudio voice-F1.mp3")).not.toBeNull();
});

it("pauses and resumes the same file without reloading it", async () => {
  await render(["voice-F1.mp3"]);
  act(() => button("playAudio voice-F1.mp3").click());
  const source = audio().getAttribute("src");
  Object.defineProperty(audio(), "paused", { configurable: true, get: () => false });
  act(() => {
    audio().dispatchEvent(new Event("play"));
  });

  act(() => button("pauseAudio voice-F1.mp3").click());

  expect(HTMLMediaElement.prototype.pause).toHaveBeenCalledTimes(1);
  expect(play).toHaveBeenCalledTimes(1);
  Object.defineProperty(audio(), "paused", { configurable: true, get: () => true });
  act(() => {
    audio().dispatchEvent(new Event("pause"));
  });
  act(() => button("playAudio voice-F1.mp3").click());
  expect(play).toHaveBeenCalledTimes(2);
  expect(audio().getAttribute("src")).toBe(source);
});

it("plays a finished file again with one click", async () => {
  await render(["voice-F1.mp3"]);
  act(() => button("playAudio voice-F1.mp3").click());
  act(() => {
    audio().dispatchEvent(new Event("play"));
  });
  // The order Chrome reports at the natural end: pause, with paused true, then ended.
  act(() => {
    audio().dispatchEvent(new Event("pause"));
    audio().dispatchEvent(new Event("ended"));
  });

  act(() => button("playAudio voice-F1.mp3").click());

  expect(HTMLMediaElement.prototype.pause).not.toHaveBeenCalled();
  expect(play).toHaveBeenCalledTimes(2);
});

it("ignores a start the browser refused or a switch interrupted", async () => {
  await render(["voice-F1.mp3", "voice-F2.mp3"]);
  play.mockRejectedValueOnce(new DOMException("interrupted", "AbortError"));

  await act(async () => button("playAudio voice-F1.mp3").click());
  await act(async () => button("playAudio voice-F2.mp3").click());

  expect(host.textContent).not.toContain("audioPlaybackUnavailable");
  // The highlighted chip names the clip; the player names it for assistive technology.
  expect(audio().getAttribute("aria-label")).toBe("voice-F2.mp3");
});

it("names the file that failed, keeps its download, and lets it be tried again", async () => {
  await render(["broken.mp3", "voice-F2.mp3"]);
  act(() => button("playAudio broken.mp3").click());
  Object.defineProperty(audio(), "error", { configurable: true, get: () => ({ code: 4 }) });

  act(() => {
    audio().dispatchEvent(new Event("error"));
  });

  expect(host.textContent).toContain("audioPlaybackUnavailable broken.mp3");
  // There is no visible player: the chips alone control playback.
  expect(audio().hidden).toBe(true);
  expect(audio().hasAttribute("controls")).toBe(false);
  expect(host.querySelector('a[download="broken.mp3"]')).not.toBeNull();

  // A dropped connection reports the same event, so the file can be tried again.
  Object.defineProperty(audio(), "error", { configurable: true, get: () => null });
  act(() => button("playAudio broken.mp3").click());

  expect(host.textContent).not.toContain("audioPlaybackUnavailable");
  expect(play).toHaveBeenCalledTimes(2);
  expect(audio().hidden).toBe(true);
});

it("does not blame the current file for an error event with no media error", async () => {
  await render(["voice-F1.mp3", "voice-F2.mp3"]);
  act(() => button("playAudio voice-F1.mp3").click());
  act(() => button("playAudio voice-F2.mp3").click());

  act(() => {
    audio().dispatchEvent(new Event("error"));
  });

  expect(host.textContent).not.toContain("audioPlaybackUnavailable");
  expect(audio().hidden).toBe(true);
});

it("pauses other message media when it starts, and leaves a voice call alone", async () => {
  const video = document.createElement("video");
  video.setAttribute("data-message-media", "");
  const voice = document.createElement("audio");
  document.body.append(video, voice);
  for (const media of [video, voice]) {
    Object.defineProperty(media, "paused", { configurable: true, get: () => false });
  }
  await render(["voice-F1.mp3"]);
  act(() => button("playAudio voice-F1.mp3").click());
  const pause = vi.mocked(HTMLMediaElement.prototype.pause);
  pause.mockClear();

  act(() => {
    audio().dispatchEvent(new Event("play"));
  });

  expect(pause.mock.contexts).toEqual([video]);
  video.remove();
  voice.remove();
});
