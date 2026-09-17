// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vite-plus/test";
import { useSidePanelLayout } from "./useSidePanelLayout";
import { getChatSession, useUIStore } from "../stores/useUIStore";
import type { SidePanelSurface } from "./useSidePanelSelection";

let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
let layout: ReturnType<typeof useSidePanelLayout>;
let frames: Map<number, FrameRequestCallback>;
let nextFrame: number;
const flushFrames = () =>
  act(async () => {
    const pending = [...frames.values()];
    frames.clear();
    pending.forEach((callback) => callback(0));
  });
function Harness({
  group = "a",
  surface = "presentation",
}: {
  group?: string;
  surface?: SidePanelSurface;
}) {
  const ref = useRef<HTMLDivElement>(null);
  layout = useSidePanelLayout(group, surface, false, ref);
  return (
    <div ref={ref}>
      <div
        data-divider
        onPointerDown={layout.onPointerDown}
        onKeyDown={layout.onKeyDown}
        tabIndex={0}
      />
    </div>
  );
}
const render = (group = "a", surface: SidePanelSurface = "presentation") =>
  act(async () => root.render(<Harness group={group} surface={surface} />));
const state = (group = "a") => getChatSession(group, useUIStore.getState().chatSessions);
const pointer = async (type: string, x: number, flush = true) => {
  await act(async () => {
    const target = type === "pointerdown" ? host.querySelector("[data-divider]")! : window;
    target.dispatchEvent(
      new PointerEvent(type, { clientX: x, button: 0, bubbles: true, cancelable: true }),
    );
  });
  if (flush) await flushFrames();
};
const key = (key: string) =>
  act(async () =>
    host
      .querySelector("[data-divider]")!
      .dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true })),
  );
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  frames = new Map();
  nextFrame = 0;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000);
  localStorage.clear();
  useUIStore.setState({ chatSessions: {} });
  // Existing expanded preferences keep their resize behavior after the new default.
  useUIStore.getState().setChatSidePanelLayout("a", { compact: false });
  useUIStore.getState().setChatSidePanelLayout("b", { compact: false });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
it("resizes Files too, saves on release and restores each Group independently", async () => {
  await render("a", "files");
  await pointer("pointerdown", 500);
  await pointer("pointermove", 400);
  expect(layout.width).toBe(460);
  expect(state().sidePanelWidth).toBe(360);
  await pointer("pointerup", 400);
  expect(state().sidePanelWidth).toBe(460);
  await render("b", "files");
  expect(layout.width).toBe(360);
  await render("a", "files");
  expect(layout.width).toBe(460);
  expect(JSON.parse(localStorage.getItem("cccc-chat-sessions")!).a.sidePanelWidth).toBe(460);
});
it("snaps into compact slots without shrinking Files or forgetting the expanded width", async () => {
  await render();
  await pointer("pointerdown", 500);
  await pointer("pointermove", 730);
  expect(layout.compact).toBe(true);
  expect(layout.width).toBe(64);
  await pointer("pointermove", 660);
  expect(layout.compact).toBe(true);
  await pointer("pointerup", 660);
  expect(state().presentationCompact).toBe(true);
  expect(state().sidePanelWidth).toBe(360);
  await render("a", "files");
  expect(layout.width).toBe(360);
  expect(layout.compact).toBe(false);
  await render();
  expect(layout.compact).toBe(true);
  await key("ArrowLeft");
  expect(layout.width).toBe(360);
  expect(state().presentationCompact).toBe(false);
  await key("Enter");
  expect(layout.compact).toBe(true);
});
it("cancels drags on pointer cancellation and Group switches without saving stale layout", async () => {
  await render();
  await pointer("pointerdown", 500);
  await pointer("pointermove", 400);
  await pointer("pointercancel", 400);
  expect(layout.width).toBe(360);
  await pointer("pointerdown", 500);
  await pointer("pointermove", 730);
  await render("b");
  await pointer("pointerup", 730);
  expect(state("a").presentationCompact).toBe(false);
  expect(state("b").presentationCompact).toBe(false);
  expect(document.body.style.cursor).toBe("");
  expect(document.body.style.userSelect).toBe("");
});

it("lets keyboard resizing collapse at the minimum and restore the saved width", async () => {
  await render();
  for (let step = 0; step < 4; step++) await key("ArrowRight");
  expect(layout.width).toBe(280);
  await key("ArrowRight");
  expect(layout.compact).toBe(true);
  await key("ArrowLeft");
  expect(layout.width).toBe(280);
  await render("a", "files");
  await key("ArrowRight");
  expect(layout.width).toBe(280);
  expect(layout.compact).toBe(false);
});

it("coalesces movement but commits the latest position when released before the next frame", async () => {
  await render("a", "files");
  await pointer("pointerdown", 500);
  await pointer("pointermove", 470, false);
  await pointer("pointermove", 430, false);
  expect(frames.size).toBe(1);
  expect(layout.width).toBe(360);
  await flushFrames();
  expect(layout.width).toBe(430);
  expect(state().sidePanelWidth).toBe(360);
  await pointer("pointermove", 410, false);
  await pointer("pointerup", 410);
  expect(state().sidePanelWidth).toBe(450);
  expect(frames.size).toBe(0);
  expect(layout.dragging).toBe(false);

  await pointer("pointerdown", 500);
  await pointer("pointermove", 400, false);
  await render("b", "files");
  expect(frames.size).toBe(0);
  await flushFrames();
  expect(layout.width).toBe(360);
  expect(state("a").sidePanelWidth).toBe(450);
  expect(state("b").sidePanelWidth).toBe(360);
});

it("starts new Groups compact and keeps saved Group preferences", async () => {
  await render("new");
  expect(layout.compact).toBe(true);
  expect(layout.width).toBe(64);
  expect(state("new").presentationDisplayMode).toBe("split");
  await render("a");
  expect(layout.compact).toBe(false);
  useUIStore.getState().setChatPresentationDisplayMode("a", "modal");
  await render("new");
  await render("a");
  expect(state("a").presentationDisplayMode).toBe("modal");
});
