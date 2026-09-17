// @vitest-environment happy-dom
import type { Window as HappyWindow } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { PresentationViewerModal, PresentationViewerSplitPanel } from "./PresentationViewerModal";
import type { GroupPresentation } from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

describe("Presentation refresh", () => {
  let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
  const image = () => host.querySelector<HTMLImageElement>("[data-graphic-viewer] img")!;
  const viewport = () => host.querySelector<HTMLDivElement>("[data-graphic-viewer] [role=region]")!;
  const width = () => parseFloat(host.querySelector<HTMLElement>(".select-none")!.style.width);
  const button = (name: string) => host.querySelector<HTMLButtonElement>(`[aria-label="${name}"]`)!;
  const loadImage = async () => {
    await act(async () => image().dispatchEvent(new Event("load")));
  };
  async function render(
    split = false,
    groupId = "g",
    slotId = "slot-1",
    revision = "one",
    cardType: "image" | "pdf" | "web_preview" = "image",
  ) {
    const presentation: GroupPresentation = {
      v: 1,
      slots: ["slot-1", "slot-2"].map((id, index) => ({
        slot_id: id,
        index: index + 1,
        card: {
          slot_id: id,
          title: "Drawing",
          card_type: cardType,
          published_at: revision,
          published_by: "worker",
          content: {
            mode: "workspace_link",
            workspace_rel_path:
              cardType === "pdf"
                ? "report.pdf"
                : cardType === "web_preview"
                  ? "report.html"
                  : "drawing.png",
          },
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
    if (cardType === "image") await loadImage();
  }

  let nextUrl = 0;
  beforeEach(() => {
    nextUrl = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(new Blob(["image"]), { status: 200 })),
    );
    vi.spyOn(URL, "createObjectURL").mockImplementation(() => `blob:preview-${++nextUrl}`);
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    vi.spyOn(HTMLImageElement.prototype, "decode").mockResolvedValue(undefined);
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    (window as unknown as HappyWindow).happyDOM.settings.navigation.disableChildFrameNavigation =
      true;
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
    (window as unknown as HappyWindow).happyDOM.settings.navigation.disableChildFrameNavigation =
      false;
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

  it("lets a slow refresh finish instead of restarting it on every polling tick", async () => {
    await render();
    let respond!: (response: Response) => void;
    vi.mocked(fetch).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          respond = resolve;
        }),
    );
    const oldSrc = image().src;
    await act(async () => vi.advanceTimersByTime(5000));
    const count = vi.mocked(fetch).mock.calls.length;
    const signal = vi.mocked(fetch).mock.calls.at(-1)![1]!.signal!;
    await act(async () => vi.advanceTimersByTime(15000));
    expect(fetch).toHaveBeenCalledTimes(count);
    expect(signal.aborted).toBe(false);
    expect(image().src).toBe(oldSrc);
    await act(async () => respond(new Response(new Blob(["next image"]))));
    expect(image().src).not.toBe(oldSrc);
  });

  it.each([
    [false, "pdf"],
    [true, "pdf"],
    [false, "web_preview"],
    [true, "web_preview"],
  ] as const)(
    "keeps a reading document mounted until refresh or publication (split=%s, type=%s)",
    async (split, type) => {
      await render(split, "g", "slot-1", "one", type);
      const frame = host.querySelector("iframe")!;
      const src = frame.src;
      await act(async () => vi.advanceTimersByTime(15000));
      expect(host.querySelector("iframe")).toBe(frame);
      expect(frame.src).toBe(src);

      await act(async () => button("presentationRefreshAction").click());
      const refreshed = host.querySelector("iframe")!.src;
      expect(refreshed).not.toBe(src);
      await act(async () => vi.advanceTimersByTime(10000));
      expect(host.querySelector("iframe")!.src).toBe(refreshed);

      await render(split, "g", "slot-1", "two", type);
      expect(host.querySelector("iframe")!.src).not.toBe(refreshed);
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
    const displayed = image().src;
    vi.mocked(fetch).mockResolvedValueOnce(new Response("unavailable", { status: 503 }));
    await act(async () => vi.advanceTimersByTime(5000));
    expect(host.textContent).toContain("presentationRefreshFailed");
    expect(image().src).toBe(displayed);
    expect(button("graphicViewer.actual").disabled).toBe(false);
    expect(node.querySelector(".select-none")!.parentElement).toBe(canvas);
    expect(width()).toBe(2400);
    expect(node.scrollLeft).toBe(400);
    expect(node.scrollTop).toBe(500);
    await act(async () => vi.advanceTimersByTime(5000));
    await loadImage();
    expect(host.textContent).not.toContain("presentationRefreshFailed");
    expect(button("graphicViewer.actual").disabled).toBe(false);
    expect(width()).toBe(2400);
    expect(node.scrollLeft).toBe(400);
    expect(node.scrollTop).toBe(500);
  });
});
