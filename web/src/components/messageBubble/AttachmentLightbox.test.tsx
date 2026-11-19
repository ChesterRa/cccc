// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import {
  type AttachmentPreviewItem,
  attachmentSetKey,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";
import { useUIStore } from "../../stores/useUIStore";
import { AttachmentLightbox } from "./AttachmentLightbox";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
// What the attachment looks like is the panel's own view; this checks the frame around it.
vi.mock("../resourcePanel/views/AttachmentResourceView", () => ({
  AttachmentResourceView: (props: { resourceKey: string; active: boolean; title: string }) => (
    <div data-view={props.resourceKey} data-active={String(props.active)} tabIndex={0}>
      {props.title}
    </div>
  ),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const item = (index: number, media: "image" | "pdf" | "video"): AttachmentPreviewItem => ({
  resource: {
    kind: "attachment",
    messageKey: "m1",
    eventId: "m1",
    index,
    blobGroupId: "g1",
    blobPath: `state/blobs/${index}`,
    media,
    isSvg: false,
  },
  title: `file-${index}`,
});
const items = [item(0, "video"), item(2, "pdf"), item(4, "image")];

let root: Root;
let host: HTMLDivElement;
let opener: HTMLButtonElement;

beforeEach(() => {
  useUIStore.setState({ chatObscuringViews: 0 });
  useAttachmentPreviewStore.setState({ sets: {}, viewing: null });
  useResourcePanelStore.setState({ groups: {}, shownGroupId: "" });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  // The message's attachment control that opens the preview.
  opener = document.createElement("button");
  document.body.append(opener);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  opener.remove();
  vi.unstubAllGlobals();
});

const render = (groupId = "g1", canOpenInPanel = true) =>
  act(() =>
    root.render(
      <AttachmentLightbox groupId={groupId} isDark={false} canOpenInPanel={canOpenInPanel} />,
    ),
  );
const show = (index: number) => {
  opener.focus();
  act(() => useAttachmentPreviewStore.getState().view("g1", "m1", items, index));
  render();
};
const position = () => document.querySelector("[data-attachment-position]")?.textContent;
const key = (target: Element, init: KeyboardEventInit) =>
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
  });

it("steps with the keys and the arrows, stopping at either end", () => {
  show(0);
  const previous = document.querySelector('[data-attachment-step="previous"]')!;
  const next = document.querySelector<HTMLButtonElement>('[data-attachment-step="next"]')!;
  // The keys stand on either side of the attachment; where it stands goes beside its name.
  const body = document.querySelector("[data-media-body]")!;
  expect(
    [...body.children].map((child) => child.getAttribute("data-media-step") ?? "view"),
  ).toEqual(["previous", "view", "next"]);
  expect(document.querySelector("[data-media-title] [data-attachment-position]")).not.toBeNull();
  expect(position()).toBe("1/3");
  expect(previous.getAttribute("aria-disabled")).toBe("true");
  // The clicked video plays by itself; one reached by stepping waits.
  expect(document.querySelector("[data-view]")?.getAttribute("data-active")).toBe("true");

  act(() => next.click());
  expect(position()).toBe("2/3");
  expect(document.querySelector("[data-view]")?.getAttribute("data-view")).toBe("attachment:m1:2");

  key(next, { key: "ArrowRight" });
  expect(position()).toBe("3/3");
  expect(next.getAttribute("aria-disabled")).toBe("true");
  key(next, { key: "ArrowRight" });
  act(() => next.click());
  expect(position()).toBe("3/3");

  // A modified arrow belongs to the browser.
  key(next, { key: "ArrowLeft", altKey: true });
  expect(position()).toBe("3/3");
  key(next, { key: "ArrowLeft" });
  key(next, { key: "ArrowLeft" });
  expect(position()).toBe("1/3");
  expect(document.querySelector("[data-view]")?.getAttribute("data-active")).toBe("false");
});

it("leaves the arrows to a document or video, but steps from an image", () => {
  show(2);
  const view = () => document.querySelector("[data-view]")!;
  key(view(), { key: "ArrowRight" });
  expect(position()).toBe("2/3");
  act(() => document.querySelector<HTMLButtonElement>('[data-attachment-step="next"]')!.click());
  expect(position()).toBe("3/3");
  key(view(), { key: "ArrowLeft" });
  expect(position()).toBe("2/3");
});

it("closes with Esc and gives focus back to what opened it", () => {
  show(0);
  act(() => document.querySelector<HTMLButtonElement>('[data-attachment-step="next"]')!.click());
  key(document.querySelector('[data-attachment-step="next"]')!, { key: "Escape" });
  expect(useAttachmentPreviewStore.getState().viewing).toBeNull();
  expect(document.querySelector("[data-media-lightbox]")).toBeNull();
  expect(document.activeElement).toBe(opener);
});

it("moves the attachment into a panel tab, keeping the message's list for the reader", () => {
  show(2);
  act(() => document.querySelector<HTMLButtonElement>("[data-attachment-open-in-panel]")!.click());
  const panel = useResourcePanelStore.getState().groups.g1;
  expect(panel.tabs.map((tab) => tab.key)).toEqual(["attachment:m1:2"]);
  expect(panel.open).toBe(true);
  expect(useAttachmentPreviewStore.getState().viewing).toBeNull();
  expect(useAttachmentPreviewStore.getState().sets[attachmentSetKey("g1", "m1")]).toHaveLength(3);
  expect(document.querySelector("[data-media-lightbox]")).toBeNull();
});

it("closes when another conversation is shown", () => {
  show(0);
  render("g2");
  expect(useAttachmentPreviewStore.getState().viewing).toBeNull();
  expect(document.querySelector("[data-media-lightbox]")).toBeNull();
});

it("offers no panel key where the panel would cover the messages too", () => {
  show(0);
  render("g1", false);
  expect(document.querySelector("[data-attachment-open-in-panel]")).toBeNull();
});
