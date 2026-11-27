// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi, type MockInstance } from "vite-plus/test";
import type { MessageAttachment } from "../../types";
import { AudioAttachments } from "./AudioAttachments";
import { formatAudioTime } from "./messageMedia";

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

it("formats audio durations for seconds, minutes, hours, and non-finite values", () => {
  expect(formatAudioTime(0)).toBe("0:00");
  expect(formatAudioTime(-3)).toBe("0:00");
  expect(formatAudioTime(Number.NaN)).toBe("0:00");
  expect(formatAudioTime(Number.POSITIVE_INFINITY)).toBe("0:00");
  expect(formatAudioTime(8.9)).toBe("0:08");
  expect(formatAudioTime(65)).toBe("1:05");
  expect(formatAudioTime(3661)).toBe("1:01:01");
});

it("shows duration and lets the active clip seek by click, drag, and keyboard without changing inactive chips", async () => {
  await render(["long-note.mp3", "second-note.mp3"]);
  act(() => button("playAudio long-note.mp3").click());

  let timeValue = 0;
  Object.defineProperty(audio(), "duration", { value: 120, configurable: true });
  Object.defineProperty(audio(), "currentTime", {
    configurable: true,
    get: () => timeValue,
    set: (value: number) => {
      timeValue = value;
    },
  });
  act(() => audio().dispatchEvent(new Event("loadedmetadata")));

  const sliders = host.querySelectorAll<HTMLDivElement>("[data-message-audio-seek]");
  expect(sliders).toHaveLength(1);
  const slider = sliders[0];
  expect(slider.getAttribute("role")).toBe("slider");
  expect(slider.getAttribute("aria-label")).toBe("seekAudio long-note.mp3");
  expect(slider.closest("[data-message-audio]")?.textContent).toContain("long-note.mp3");
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("0:00 / 2:00");
  expect(slider.closest("button")).toBeNull();

  // Click / drag along the 200px track: preview during drag, commit on pointer up.
  vi.spyOn(slider, "getBoundingClientRect").mockReturnValue({
    left: 100,
    width: 200,
    right: 300,
    top: 0,
    bottom: 24,
    height: 24,
    x: 100,
    y: 0,
    toJSON: () => ({}),
  });

  act(() => {
    slider.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, pointerId: 1, button: 0, clientX: 150 }),
    );
  });
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("0:30 / 2:00");
  expect(timeValue).toBe(0);

  // Intermediate timeupdate during drag does not snap the preview back.
  timeValue = 5;
  act(() => audio().dispatchEvent(new Event("timeupdate")));
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("0:30 / 2:00");

  act(() => {
    slider.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, pointerId: 1, clientX: 250 }),
    );
  });
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("1:30 / 2:00");

  act(() => {
    slider.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, pointerId: 1, clientX: 250 }),
    );
  });
  expect(timeValue).toBe(90);
  expect(host.querySelector<HTMLElement>("[data-message-audio-progress]")?.style.width).toBe("75%");
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("1:30 / 2:00");

  // Pointer cancel clears the drag preview without changing currentTime.
  act(() => {
    slider.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, pointerId: 2, button: 0, clientX: 100 }),
    );
  });
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("0:00 / 2:00");
  act(() => {
    slider.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: 2 }));
  });
  expect(timeValue).toBe(90);
  expect(host.querySelector("[data-message-audio-time]")?.textContent).toBe("1:30 / 2:00");

  // Keyboard stepping: ArrowLeft (-5s), ArrowRight (+5s), PageDown (-15s), PageUp (+15s), Home, End.
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowLeft" }));
  });
  expect(timeValue).toBe(85);
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }));
  });
  expect(timeValue).toBe(90);
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "PageDown" }));
  });
  expect(timeValue).toBe(75);
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "PageUp" }));
  });
  expect(timeValue).toBe(90);
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Home" }));
  });
  expect(timeValue).toBe(0);
  act(() => {
    slider.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "End" }));
  });
  expect(timeValue).toBe(120);

  // Switching clips resets duration and removes the old seek bar until the new clip loads metadata.
  act(() => button("playAudio second-note.mp3").click());
  expect(host.querySelectorAll("[data-message-audio-seek]")).toHaveLength(0);
  expect(host.querySelector("[data-message-audio-time]")).toBeNull();
});

it("hides the seek slider and time readout for non-finite streams", async () => {
  await render(["live-stream.mp3"]);
  act(() => button("playAudio live-stream.mp3").click());
  Object.defineProperty(audio(), "duration", {
    value: Number.POSITIVE_INFINITY,
    configurable: true,
  });
  act(() => audio().dispatchEvent(new Event("durationchange")));
  expect(host.querySelector("[data-message-audio-seek]")).toBeNull();
  expect(host.querySelector("[data-message-audio-time]")).toBeNull();
  expect(host.querySelector<HTMLElement>("[data-message-audio-progress]")?.style.width).toBe("0%");
});

it("expands only the selected chip when the same audio file is attached twice in one message", async () => {
  await render(["dup.mp3", "dup.mp3"]);
  const chips = () => [...host.querySelectorAll<HTMLElement>("[data-message-audio]")];
  const toggles = () => [
    ...host.querySelectorAll<HTMLButtonElement>("[data-message-audio-toggle]"),
  ];
  expect(chips()).toHaveLength(2);

  let timeValue = 0;
  Object.defineProperty(audio(), "duration", { value: 60, configurable: true });
  Object.defineProperty(audio(), "currentTime", {
    configurable: true,
    get: () => timeValue,
    set: (value: number) => {
      timeValue = value;
    },
  });

  act(() => toggles()[0].click());
  act(() => audio().dispatchEvent(new Event("loadedmetadata")));
  expect(host.querySelectorAll("[data-message-audio-seek]")).toHaveLength(1);
  expect(chips()[0].querySelector("[data-message-audio-seek]")).not.toBeNull();
  expect(chips()[1].querySelector("[data-message-audio-seek]")).toBeNull();

  // Switching to the second duplicate entry moves the single seek bar to it and restarts at 0:00.
  timeValue = 25;
  act(() => toggles()[1].click());
  expect(timeValue).toBe(0);
  expect(host.querySelectorAll("[data-message-audio-seek]")).toHaveLength(1);
  expect(chips()[0].querySelector("[data-message-audio-seek]")).toBeNull();
  expect(chips()[1].querySelector("[data-message-audio-seek]")).not.toBeNull();
  expect(host.querySelectorAll("audio")).toHaveLength(1);
});
