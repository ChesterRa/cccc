// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MediaLightbox } from "./MediaLightbox";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it("wraps focus that leaves the player forward, but lets a backward pass go through", () => {
  act(() =>
    root.render(
      <MediaLightbox
        open
        onClose={() => {}}
        dialogLabel="Video player"
        title="clip.mp4"
        downloadHref="/download/clip.mp4"
        downloadName="clip.mp4"
      >
        <video controls />
      </MediaLightbox>,
    ),
  );
  const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
  const guard = dialog.querySelector<HTMLElement>("[data-focus-guard]")!;
  const download = dialog.querySelector<HTMLAnchorElement>('a[download="clip.mp4"]')!;

  // Tabbing past the player's last control.
  act(() => guard.focus());
  expect(document.activeElement).toBe(download);

  // Shift+Tab from the start passes through toward the player's controls.
  guard.dataset.focusPassThrough = "true";
  act(() => guard.focus());
  expect(document.activeElement).toBe(guard);
  expect(guard.dataset.focusPassThrough).toBeUndefined();
});
