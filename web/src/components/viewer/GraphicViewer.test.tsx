// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, afterEach, describe, expect, it, vi } from "vite-plus/test";
import { GraphicViewer } from "./GraphicViewer";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("static graphic navigation", () => {
  let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
  let resize: (width: number, height: number) => void;
  const svg = '<svg viewBox="0 0 12000 8000"></svg>';
  const region = () => host.querySelector<HTMLDivElement>('[role="region"]')!;
  const button = (name: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="graphicViewer.${name}"]`)!;
  const width = () =>
    Number.parseFloat(region().querySelector<HTMLElement>(".select-none")!.style.width);
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          resize = (width, height) =>
            callback(
              [{ contentRect: { width, height } } as ResizeObserverEntry],
              this as unknown as ResizeObserver,
            );
        }
        observe() {
          resize(500, 400);
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
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  it("fits a large graphic, zooms around the viewport, and resets for a new resource", async () => {
    await act(async () =>
      root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
    );
    expect(width()).toBeCloseTo(468);
    await act(async () => button("actual").click());
    expect(width()).toBe(12000);
    expect(region().scrollLeft).toBeGreaterThan(5000);
    await act(async () => button("out").click());
    expect(width()).toBe(9600);
    await act(async () => button("fit").click());
    expect(width()).toBeCloseTo(468);
    await act(async () => button("actual").click());
    await act(async () =>
      root.render(
        <GraphicViewer
          svg={svg.replace("8000", "9000")}
          width={12000}
          height={9000}
          alt="Next map"
        />,
      ),
    );
    expect(width()).toBeCloseTo(468);
    expect(region().scrollLeft).toBe(0);
  });
  it("leaves wheel scrolling native and uses explicit controls to zoom", async () => {
    await act(async () =>
      root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
    );
    const wheel = new WheelEvent("wheel", {
      deltaY: -80,
      clientX: 250,
      clientY: 200,
      bubbles: true,
      cancelable: true,
    });
    await act(async () => region().dispatchEvent(wheel));
    expect(wheel.defaultPrevented).toBe(false);
    expect(width()).toBeCloseTo(468);
    await act(async () => button("in").click());
    expect(width()).toBeGreaterThan(468);
    await act(async () =>
      region().dispatchEvent(
        new KeyboardEvent("keydown", { key: "0", bubbles: true, cancelable: true }),
      ),
    );
    expect(width()).toBeCloseTo(468);
    const outside = new WheelEvent("wheel", { deltaY: 100, bubbles: true, cancelable: true });
    host.dispatchEvent(outside);
    expect(outside.defaultPrevented).toBe(false);
  });
  it.each(["ctrlKey", "metaKey"] as const)(
    "zooms at the pointer for %s+wheel without intercepting ordinary scrolling",
    async (modifier) => {
      await act(async () =>
        root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
      );
      const modified = new WheelEvent("wheel", {
        ctrlKey: true,
        deltaY: -100,
        clientX: 300,
        clientY: 180,
        bubbles: true,
        cancelable: true,
      });
      // happy-dom's WheelEvent constructor does not populate modifier keys.
      Object.defineProperty(modified, modifier, { value: true });
      await act(async () => region().dispatchEvent(modified));
      expect(modified.defaultPrevented).toBe(true);
      expect(width()).toBeGreaterThan(468);
      const plain = new WheelEvent("wheel", { deltaY: 100, bubbles: true, cancelable: true });
      await act(async () => region().dispatchEvent(plain));
      expect(plain.defaultPrevented).toBe(false);
    },
  );
  it("accumulates modified wheel input delivered before a render", async () => {
    await act(async () =>
      root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
    );
    await act(async () => {
      for (let index = 0; index < 4; index += 1) {
        const wheel = new WheelEvent("wheel", {
          deltaY: -100,
          clientX: 250,
          clientY: 200,
          bubbles: true,
          cancelable: true,
        });
        Object.defineProperty(wheel, "ctrlKey", { value: true });
        region().dispatchEvent(wheel);
      }
    });
    expect(width()).toBeCloseTo(468 * Math.exp(0.8));
  });
  it("keeps fit responsive when rapid opposing wheel input cancels out", async () => {
    await act(async () =>
      root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
    );
    await act(async () => {
      for (const deltaY of [-100, 100]) {
        const wheel = new WheelEvent("wheel", {
          deltaY,
          clientX: 250,
          clientY: 200,
          bubbles: true,
          cancelable: true,
        });
        Object.defineProperty(wheel, "ctrlKey", { value: true });
        region().dispatchEvent(wheel);
      }
    });
    expect(width()).toBeCloseTo(468);
    await act(async () => resize(400, 400));
    expect(width()).toBeCloseTo(368);
  });
  it("toggles on a click, distinguishes a drag, and closes only an unmoved blank click", async () => {
    const close = vi.fn();
    await act(async () =>
      root.render(
        <GraphicViewer svg={svg} width={12000} height={8000} alt="Map" onBlankClick={close} />,
      ),
    );
    region().setPointerCapture = vi.fn();
    const image = region().querySelector<HTMLElement>("[data-graphic-image]")!;
    const pointer = (target: Element, type: string, x: number, y: number) =>
      target.dispatchEvent(
        new PointerEvent(type, {
          pointerId: 1,
          pointerType: "mouse",
          button: 0,
          clientX: x,
          clientY: y,
          bubbles: true,
          cancelable: true,
        }),
      );
    await act(async () => {
      pointer(image, "pointerdown", 100, 100);
      pointer(region(), "pointerup", 100, 100);
    });
    expect(width()).toBe(12000);
    await act(async () => {
      pointer(image, "pointerdown", 100, 100);
      pointer(region(), "pointermove", 130, 100);
      pointer(region(), "pointerup", 130, 100);
      pointer(region(), "pointerdown", 40, 40);
      pointer(region(), "pointermove", 70, 40);
      pointer(region(), "pointerup", 70, 40);
    });
    expect(width()).toBe(12000);
    expect(close).not.toHaveBeenCalled();
    await act(async () => {
      pointer(region(), "pointerdown", 40, 40);
      pointer(region(), "pointerup", 40, 40);
    });
    expect(close).toHaveBeenCalledOnce();
  });
  it("requires two touch taps to toggle and leaves SVG links alone", async () => {
    const linked = '<svg viewBox="0 0 12000 8000"><a href="#detail"><text>Detail</text></a></svg>';
    await act(async () =>
      root.render(<GraphicViewer svg={linked} width={12000} height={8000} alt="Map" />),
    );
    region().setPointerCapture = vi.fn();
    const image = region().querySelector<HTMLElement>("[data-graphic-image]")!;
    const pointer = (target: Element, type: string) =>
      target.dispatchEvent(
        new PointerEvent(type, {
          pointerId: 2,
          pointerType: "touch",
          button: 0,
          clientX: 100,
          clientY: 100,
          bubbles: true,
          cancelable: true,
        }),
      );
    await act(async () => {
      pointer(image, "pointerdown");
      pointer(region(), "pointerup");
    });
    expect(width()).toBeCloseTo(468);
    await act(async () => {
      pointer(image, "pointerdown");
      pointer(region(), "pointerup");
    });
    expect(width()).toBe(12000);
    const linkDown = new PointerEvent("pointerdown", {
      pointerId: 3,
      pointerType: "mouse",
      button: 0,
      bubbles: true,
      cancelable: true,
    });
    await act(async () => region().querySelector("a")!.dispatchEvent(linkDown));
    expect(linkDown.defaultPrevented).toBe(false);
    expect(width()).toBe(12000);
  });
  it("does not turn the end of a pinch into a tap or a blank-space close", async () => {
    const close = vi.fn();
    await act(async () =>
      root.render(
        <GraphicViewer svg={svg} width={12000} height={8000} alt="Map" onBlankClick={close} />,
      ),
    );
    region().setPointerCapture = vi.fn();
    const image = region().querySelector<HTMLElement>("[data-graphic-image]")!;
    const pointer = (target: Element, type: string, id: number, x: number) =>
      target.dispatchEvent(
        new PointerEvent(type, {
          pointerId: id,
          pointerType: "touch",
          button: 0,
          clientX: x,
          clientY: 100,
          bubbles: true,
          cancelable: true,
        }),
      );
    await act(async () => {
      pointer(image, "pointerdown", 1, 100);
      pointer(image, "pointerdown", 2, 160);
      pointer(region(), "pointermove", 2, 180);
      pointer(region(), "pointermove", 2, 220);
      pointer(region(), "pointerup", 1, 100);
      pointer(region(), "pointerup", 2, 220);
    });
    expect(width()).toBeCloseTo(936);
    expect(close).not.toHaveBeenCalled();
  });
  it("upscales known SVGs when fitting while leaving small raster images at their natural size", async () => {
    await act(async () => root.render(<GraphicViewer src="/small.png" alt="Small" />));
    const image = host.querySelector("img")!;
    Object.defineProperties(image, {
      naturalWidth: { configurable: true, value: 120 },
      naturalHeight: { configurable: true, value: 80 },
    });
    await act(async () => image.dispatchEvent(new Event("load")));
    expect(width()).toBe(120);
    await act(async () => root.render(<GraphicViewer src="/small.png" alt="Small" isSvg />));
    expect(width()).toBeCloseTo(468);
    await act(async () =>
      root.render(
        <GraphicViewer
          svg='<svg viewBox="0 0 120 80"></svg>'
          width={120}
          height={80}
          alt="Vector"
        />,
      ),
    );
    expect(width()).toBeCloseTo(468);
  });
  it("fits within fractional content bounds despite rounded client dimensions", async () => {
    await act(async () =>
      root.render(<GraphicViewer svg={svg} width={12000} height={8000} alt="Map" />),
    );
    await act(async () => resize(499.5, 399.5));
    expect(width()).toBeCloseTo(467.5);
    await act(async () => resize(499.5, 179.5));
    expect(width()).toBeCloseTo(221.25);
    await act(async () => button("actual").click());
    await act(async () => resize(489.5, 169.5));
    expect(width()).toBe(12000);
    await act(async () => button("fit").click());
    await act(async () => resize(499.5, 179.5));
    expect(width()).toBeCloseTo(221.25);
  });
  it("keeps failed images explicit and disables their controls", async () => {
    await act(async () => root.render(<GraphicViewer src="/missing.png" alt="Missing" />));
    await act(async () => host.querySelector("img")!.dispatchEvent(new Event("error")));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("imagePreviewUnavailable");
    expect(button("actual").disabled).toBe(true);
  });
});
