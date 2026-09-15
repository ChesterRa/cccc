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
