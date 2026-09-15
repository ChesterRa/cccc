// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { PresentationViewerModal, PresentationViewerSplitPanel } from "./PresentationViewerModal";
import type { GroupPresentation } from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

describe("Presentation image refresh", () => {
  let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
  const image = () => host.querySelector<HTMLImageElement>("[data-graphic-viewer] img")!;
  const viewport = () => host.querySelector<HTMLDivElement>("[data-graphic-viewer] [role=region]")!;
  const width = () => parseFloat(host.querySelector<HTMLElement>(".select-none")!.style.width);
  const button = (name: string) => host.querySelector<HTMLButtonElement>(`[aria-label="${name}"]`)!;
  const loadImage = async () => {
    await act(async () => image().dispatchEvent(new Event("load")));
  };
  async function render(split = false, groupId = "g", slotId = "slot-1", revision = "one") {
    const presentation: GroupPresentation = {
      v: 1,
      slots: ["slot-1", "slot-2"].map((id, index) => ({
        slot_id: id,
        index: index + 1,
        card: {
          slot_id: id,
          title: "Drawing",
          card_type: "image",
          published_at: revision,
          published_by: "worker",
          content: { mode: "workspace_link", workspace_rel_path: "drawing.png" },
        },
      })),
    };
    const props = { isDark: false, groupId, slotId, presentation, onClose: vi.fn() };
    await act(async () =>
      root.render(
        split ? (
          <PresentationViewerSplitPanel {...props} />
        ) : (
          <PresentationViewerModal isOpen {...props} />
        ),
      ),
    );
    await loadImage();
  }

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(640);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(480);
    vi.spyOn(HTMLImageElement.prototype, "naturalWidth", "get").mockReturnValue(2400);
    vi.spyOn(HTMLImageElement.prototype, "naturalHeight", "get").mockReturnValue(1800);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private callback: ResizeObserverCallback) {}
        observe() {
          this.callback(
            [{ contentRect: { width: 640, height: 480 } } as ResizeObserverEntry],
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
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each([false, true])(
    "retains zoom and position through automatic and manual refresh (split=%s)",
    async (split) => {
      await render(split);
      await act(async () => button("graphicViewer.actual").click());
      const node = viewport();
      node.scrollTop = 320;
      node.scrollLeft = 180;
      const wheel = new WheelEvent("wheel", { deltaY: 100, bubbles: true, cancelable: true });
      node.dispatchEvent(wheel);
      expect(wheel.defaultPrevented).toBe(false);

      for (const refresh of [
        () => vi.advanceTimersByTime(5000),
        () => button("presentationRefreshAction").click(),
      ]) {
        const src = image().getAttribute("src");
        await act(async () => {
          refresh();
        });
        expect(image().getAttribute("src")).not.toBe(src);
        await loadImage();
        expect(viewport()).toBe(node);
        expect(width()).toBe(2400);
        expect(node.scrollTop).toBe(320);
        expect(node.scrollLeft).toBe(180);
      }
    },
  );

  it("fits another Group, slot or publication even when the other identity fields match", async () => {
    await render();
    for (const [groupId, slotId, revision] of [
      ["g", "slot-2", "one"],
      ["other", "slot-2", "one"],
      ["other", "slot-2", "two"],
    ]) {
      await act(async () => button("graphicViewer.actual").click());
      const previous = viewport();
      await render(false, groupId, slotId, revision);
      expect(viewport()).not.toBe(previous);
      expect(width()).toBeLessThan(640);
      expect(viewport().scrollTop).toBe(0);
    }
  });

  it("keeps zoom and canvas extent through a failed refresh and recovery", async () => {
    await render();
    await act(async () => button("graphicViewer.actual").click());
    const node = viewport();
    const canvas = node.querySelector(".select-none")!.parentElement;
    node.scrollLeft = 400;
    node.scrollTop = 500;
    await act(async () => vi.advanceTimersByTime(5000));
    await act(async () => image().dispatchEvent(new Event("error")));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("imagePreviewUnavailable");
    expect(button("graphicViewer.actual").disabled).toBe(true);
    expect(node.querySelector(".select-none")!.parentElement).toBe(canvas);
    expect(width()).toBe(2400);
    expect(node.scrollLeft).toBe(400);
    expect(node.scrollTop).toBe(500);
    await act(async () => vi.advanceTimersByTime(5000));
    await loadImage();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(button("graphicViewer.actual").disabled).toBe(false);
    expect(width()).toBe(2400);
    expect(node.scrollLeft).toBe(400);
    expect(node.scrollTop).toBe(500);
  });
});
