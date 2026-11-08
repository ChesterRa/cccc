// @vitest-environment happy-dom
import type { Window as HappyWindow } from "happy-dom";
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { PresentationViewerModal, PresentationViewerSplitPanel } from "./PresentationViewerModal";
import type { GroupPresentation } from "../../types";
import { groupMessagesVisible, useUIStore } from "../../stores/useUIStore";

type ViewerExtras = Pick<
  ComponentProps<typeof PresentationViewerSplitPanel>,
  "onQuoteInChat" | "focusRef" | "onSelectSlot" | "visible"
> & { isOpen?: boolean; pdfUrl?: string };

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("../viewer/PdfCanvasViewer", () => ({
  PdfCanvasViewer: ({ href }: { href: string }) => <div data-pdf-canvas-viewer={href} />,
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
    extra: ViewerExtras = {},
  ) {
    const { isOpen = true, pdfUrl, ...viewerExtras } = extra;
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
          content: pdfUrl
            ? { mode: "reference", url: pdfUrl }
            : {
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
    const props = {
      isDark: false,
      groupId,
      slotId,
      presentation,
      onClose: vi.fn(),
      ...viewerExtras,
    };
    await act(async () =>
      root.render(
        split ? (
          <PresentationViewerSplitPanel {...props} />
        ) : (
          <PresentationViewerModal isOpen={isOpen} {...props} />
        ),
      ),
    );
    if (cardType === "image" && isOpen) await loadImage();
  }

  // A screen 480px tall or less, which can turn taller and back.
  const shortScreen = () => {
    let short = true;
    const listeners = new Set<() => void>();
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query) =>
        ({
          get matches() {
            return query === "(max-height: 480px)" && short;
          },
          media: query,
          addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
          removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
        }) as unknown as MediaQueryList,
    );
    return async (next: boolean) => {
      short = next;
      await act(async () => listeners.forEach((listener) => listener()));
    };
  };
  const unfoldKey = () =>
    host.querySelector<HTMLButtonElement>('[aria-label="presentationSlotsAndDetailsAction"]');

  it("blocks chat in a window but not in the split panel", async () => {
    useUIStore.setState({
      activeTab: "chat",
      chatSessions: {},
      isSmallScreen: false,
      workspaceFileViewerGroupId: "",
      chatObscuringViews: 0,
    });
    await render(false);
    expect(groupMessagesVisible("g", useUIStore.getState())).toBe(false);
    await render(true);
    expect(groupMessagesVisible("g", useUIStore.getState())).toBe(true);
    await render(false, "g", "slot-1", "one", "image", { isOpen: false });
    expect(groupMessagesVisible("g", useUIStore.getState())).toBe(true);
  });

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

  it("keeps the viewer mounted but pauses linked-image refresh while its resource tab is hidden", async () => {
    await render(true, "g", "slot-1", "one", "image", { visible: true });
    const node = viewport();
    const count = vi.mocked(fetch).mock.calls.length;
    await render(true, "g", "slot-1", "one", "image", { visible: false });
    await act(async () => vi.advanceTimersByTime(10_000));
    expect(vi.mocked(fetch)).toHaveBeenCalledTimes(count);
    expect(viewport()).toBe(node);
    await render(true, "g", "slot-1", "one", "image", { visible: true });
    await act(async () => vi.advanceTimersByTime(5_000));
    expect(vi.mocked(fetch)).toHaveBeenCalledTimes(count + 1);
    expect(viewport()).toBe(node);
  });

  it("folds the slots, the details and the quote key behind one key on a short screen", async () => {
    shortScreen();
    await render(false, "g", "slot-1", "one", "image", {
      onSelectSlot: vi.fn(),
      onQuoteInChat: vi.fn(),
    });
    await act(async () => vi.advanceTimersByTime(50));
    const toggle = unfoldKey()!;
    const folded = document.getElementById(toggle.getAttribute("aria-controls")!)!;
    expect(document.activeElement).toBe(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(folded.hidden).toBe(true);
    expect(host.querySelector("[data-presentation-slot-navigation]")).toBeNull();
    expect(host.querySelector("[data-presentation-details]")).toBeNull();
    expect(host.querySelector('[aria-label="presentationQuoteInChatAction"]')).toBeNull();

    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(folded.hidden).toBe(false);
    expect(folded.querySelector("[data-presentation-slot-navigation]")).not.toBeNull();
    expect(
      folded.querySelector(
        '[data-presentation-details] [aria-label="presentationQuoteInChatAction"]',
      ),
    ).not.toBeNull();
    expect(document.activeElement).toBe(toggle);
  });

  it("opens folded again after it was closed unfolded", async () => {
    // A window stays mounted while closed, for each slot opened in the Group.
    shortScreen();
    const show = (isOpen: boolean) =>
      render(false, "g", "slot-1", "one", "image", { onSelectSlot: vi.fn(), isOpen });
    await show(true);
    await act(async () => unfoldKey()!.click());
    expect(unfoldKey()!.getAttribute("aria-expanded")).toBe("true");
    await show(false);
    await show(true);
    expect(unfoldKey()!.getAttribute("aria-expanded")).toBe("false");
  });

  it("hands focus between the unfold key and the current slot as the screen turns", async () => {
    const turn = shortScreen();
    await render(true, "g", "slot-1", "one", "image", { onSelectSlot: vi.fn() });
    expect(document.activeElement).toBe(unfoldKey());

    await turn(false);
    expect(unfoldKey()).toBeNull();
    expect(document.activeElement?.getAttribute("aria-label")).toBe("presentationOpenSlot");

    await turn(true);
    expect(document.activeElement).toBe(unfoldKey());
  });

  it.each([false, true])(
    "puts focus in the viewer on opening when the current slot cannot take it (split=%s)",
    async (split) => {
      // A read-only viewer opened from an old quote of a slot that has since been cleared.
      const props = {
        isDark: false,
        readOnly: true,
        groupId: "g",
        slotId: "slot-1",
        presentation: { v: 1, slots: [] } as GroupPresentation,
        focusRef: {
          kind: "presentation_ref",
          slot_id: "slot-1",
          card_type: "image",
          snapshot: { path: "state/blobs/snapshot.png" },
        } as const,
        onSelectSlot: vi.fn(),
        onClose: vi.fn(),
      };
      await act(async () =>
        root.render(
          split ? (
            <PresentationViewerSplitPanel {...props} />
          ) : (
            <PresentationViewerModal isOpen {...props} />
          ),
        ),
      );
      await act(async () => vi.advanceTimersByTime(50));
      const focused = document.activeElement as HTMLButtonElement | null;
      expect(host.contains(focused)).toBe(true);
      expect(focused?.disabled).toBeFalsy();
    },
  );

  it.each([false, true])(
    "keeps focus in the viewer when the current slot cannot take it and the screen grows (split=%s)",
    async (split) => {
      // A read-only viewer opened from an old quote of a slot that has since been cleared,
      // with no message to jump to: no other key shares the unfold key's row.
      const turn = shortScreen();
      const focusRef = {
        kind: "presentation_ref",
        slot_id: "slot-1",
        card_type: "image",
        snapshot: { path: "state/blobs/snapshot.png" },
      } as const;
      const props = {
        isDark: false,
        readOnly: true,
        groupId: "g",
        slotId: "slot-1",
        presentation: { v: 1, slots: [] } as GroupPresentation,
        focusRef,
        onSelectSlot: vi.fn(),
        onClose: vi.fn(),
      };
      await act(async () =>
        root.render(
          split ? (
            <PresentationViewerSplitPanel {...props} />
          ) : (
            <PresentationViewerModal isOpen {...props} />
          ),
        ),
      );
      await act(async () => vi.advanceTimersByTime(50));
      expect(document.activeElement).toBe(unfoldKey());
      await turn(false);
      expect(document.activeElement).not.toBe(document.body);
      expect(host.contains(document.activeElement)).toBe(true);
    },
  );

  it("names each open window after its own card", async () => {
    const presentation: GroupPresentation = {
      v: 1,
      slots: ["Drawing", "Plan"].map((title, index) => ({
        slot_id: `slot-${index + 1}`,
        index: index + 1,
        card: {
          slot_id: `slot-${index + 1}`,
          title,
          card_type: "markdown",
          published_at: "one",
          published_by: "worker",
          content: { mode: "inline", markdown: title },
        },
      })),
    };
    const props = { isDark: false, groupId: "g", presentation, onClose: vi.fn() };
    await act(async () =>
      root.render(
        <>
          <PresentationViewerModal isOpen slotId="slot-1" {...props} />
          <PresentationViewerModal isOpen slotId="slot-2" {...props} />
        </>,
      ),
    );
    const names = Array.from(host.querySelectorAll('[role="dialog"]'), (dialog) => {
      const title = host.ownerDocument.getElementById(dialog.getAttribute("aria-labelledby")!);
      return title?.querySelector("span")?.textContent;
    });
    expect(names).toEqual(["Drawing", "Plan"]);
  });

  it("names the card type and publish time in the modal title for phone headers", async () => {
    await render(false, "g", "slot-1", "2026-10-07T09:19:00Z");
    const dialog = host.querySelector('[role="dialog"]')!;
    const title = host.ownerDocument.getElementById(dialog.getAttribute("aria-labelledby")!)!;
    const meta = title.querySelector("[data-presentation-title-meta]")!;
    expect(title.textContent).toContain("Drawing");
    expect(meta.textContent).toContain("presentationTypeImage");
    expect(meta.textContent).toMatch(/2026/);
    // Close stays a single control even though phones show it before the title.
    const close = host.ownerDocument.querySelectorAll('[aria-label="presentationCloseViewer"]');
    expect(close).toHaveLength(1);
  });

  it.each([false, true])(
    "moves the chat keys off the card into the window's details row on phones (split=%s)",
    async (split) => {
      const onQuoteInChat = vi.fn();
      await render(split, "g", "slot-1", "one", "image", {
        onQuoteInChat,
        focusRef: {
          kind: "presentation_ref",
          slot_id: "slot-1",
          card_type: "image",
          snapshot: { path: "state/blobs/snapshot.png" },
        },
      });
      const keys = (name: string) => [
        ...host.ownerDocument.querySelectorAll<HTMLButtonElement>(`[aria-label="${name}"]`),
      ];
      const inDetails = (key: Element) => !!key.closest("[data-presentation-details]");
      const quote = "presentationQuoteInChatAction";
      // The floating keys stay on wider screens and in the side panel.
      const floating = keys(quote).find((key) => !inDetails(key))!;
      expect(floating.parentElement!.classList.contains("max-sm:hidden")).toBe(!split);
      expect(keys(quote).filter(inDetails)).toHaveLength(split ? 0 : 1);
      if (split) return;

      const row = keys(quote).find(inDetails)!.parentElement!;
      expect(row.classList.contains("sm:hidden")).toBe(true);
      const snapshotKey = keys("presentationCompareSnapshotAction").find(inDetails)!;
      expect(snapshotKey.parentElement).toBe(row);
      expect(snapshotKey.textContent).toBe("");
      await act(async () => snapshotKey.click());
      const hide = keys("presentationHideSnapshotAction").filter(inDetails);
      expect(hide.map((key) => key.getAttribute("aria-pressed"))).toEqual(["true"]);
      await act(async () => keys(quote).find(inDetails)!.click());
      expect(onQuoteInChat).toHaveBeenCalledWith(expect.objectContaining({ slotId: "slot-1" }));
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

  it.each([false, true])(
    "uses the in-page reader for a same-origin PDF on touch and preserves it until refresh (split=%s)",
    async (split) => {
      vi.spyOn(window, "matchMedia").mockImplementation(
        (query) =>
          ({
            matches: query === "(pointer: coarse)",
            media: query,
            addEventListener: () => undefined,
            removeEventListener: () => undefined,
          }) as unknown as MediaQueryList,
      );
      await render(split, "g", "slot-1", "one", "pdf", { visible: true });
      const reader = host.querySelector<HTMLElement>("[data-pdf-canvas-viewer]")!;
      expect(reader).not.toBeNull();
      const src = reader.dataset.pdfCanvasViewer;
      await render(split, "g", "slot-1", "one", "pdf", { visible: false });
      await render(split, "g", "slot-1", "one", "pdf", { visible: true });
      expect(host.querySelector("[data-pdf-canvas-viewer]")).toBe(reader);
      await act(async () => button("presentationRefreshAction").click());
      const refreshed = host.querySelector<HTMLElement>("[data-pdf-canvas-viewer]")!;
      expect(refreshed).not.toBe(reader);
      expect(refreshed.dataset.pdfCanvasViewer).not.toBe(src);
      await render(split, "g", "slot-1", "two", "pdf");
      expect(host.querySelector("[data-pdf-canvas-viewer]")).not.toBe(refreshed);
    },
  );

  it("reloads a same-origin slot asset when a new card uses the same URL", async () => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query) =>
        ({
          matches: query === "(pointer: coarse)",
          media: query,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
        }) as unknown as MediaQueryList,
    );
    const pdfUrl = "/api/v1/groups/g/presentation/slots/slot-1/asset";
    await render(true, "g", "slot-1", "one", "pdf", { pdfUrl });
    const reader = host.querySelector("[data-pdf-canvas-viewer]");
    expect(reader).not.toBeNull();
    await render(true, "g", "slot-1", "two", "pdf", { pdfUrl });
    expect(host.querySelector("[data-pdf-canvas-viewer]")).not.toBe(reader);
  });

  it("keeps a remote PDF in the browser frame even on touch", async () => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query) =>
        ({
          matches: query === "(pointer: coarse)",
          media: query,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
        }) as unknown as MediaQueryList,
    );
    const pdfUrl = "https://example.invalid/report.pdf";
    await render(true, "g", "slot-1", "one", "pdf", { pdfUrl });
    expect(host.querySelector("[data-pdf-canvas-viewer]")).toBeNull();
    expect(host.querySelector("iframe")?.getAttribute("src")).toBe(pdfUrl);
  });

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
