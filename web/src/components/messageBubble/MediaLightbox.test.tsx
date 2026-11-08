// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { MediaLightbox } from "./MediaLightbox";
import { groupMessagesVisible, useUIStore } from "../../stores/useUIStore";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  useUIStore.setState({
    activeTab: "chat",
    chatSessions: {},
    isSmallScreen: false,
    workspaceFileViewerGroupId: "",
    chatObscuringViews: 0,
  });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

it("treats chat as hidden while an image is expanded and releases it on close", () => {
  const render = (open: boolean) =>
    act(() =>
      root.render(
        <MediaLightbox open={open} onClose={() => {}} dialogLabel="Image" title="image.png" image>
          <img src="/fixture.png" alt="Fixture" />
        </MediaLightbox>,
      ),
    );
  render(false);
  expect(groupMessagesVisible("g1", useUIStore.getState())).toBe(true);
  render(true);
  expect(document.querySelector("[data-media-lightbox]")).not.toBeNull();
  expect(groupMessagesVisible("g1", useUIStore.getState())).toBe(false);
  render(false);
  expect(groupMessagesVisible("g1", useUIStore.getState())).toBe(true);
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
  const hint = document.getElementById(dialog.getAttribute("aria-describedby")!)!;
  expect(hint.textContent).toBe("videoPreviewHint");

  // Tabbing past the player's last control.
  act(() => guard.focus());
  expect(document.activeElement).toBe(download);

  // Shift+Tab from the start passes through toward the player's controls.
  guard.dataset.focusPassThrough = "true";
  act(() => guard.focus());
  expect(document.activeElement).toBe(guard);
  expect(guard.dataset.focusPassThrough).toBeUndefined();
});
