// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vite-plus/test";
import { RuntimeDockRing } from "./RuntimeDockRing";

const host = document.createElement("div");
const root = createRoot(host);
const visibility = Object.getOwnPropertyDescriptor(document, "visibilityState");

async function render(active: boolean) {
  await act(async () =>
    root.render(
      <RuntimeDockRing
        size={44}
        active={active}
        colorClassName="text-green-500"
        gradientId="test-ring"
      />,
    ),
  );
}

afterEach(async () => {
  await act(async () => root.render(null));
  if (visibility) Object.defineProperty(document, "visibilityState", visibility);
  else Reflect.deleteProperty(document, "visibilityState");
});

it("stops background motion and resumes the active status without changing ring geometry", async () => {
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  await render(true);
  const layer = host.querySelector<HTMLElement>(".runtime-dock-ring-layer")!;
  expect(layer.tagName).toBe("SPAN");
  expect(layer.dataset.animating).toBe("true");
  expect(layer.querySelector("svg")?.getAttribute("width")).toBe("44");
  await act(async () => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(layer.dataset.animating).toBe("false");
  expect(layer.querySelector(".runtime-dock-ring__activity")).not.toBeNull();
  await act(async () => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(layer.dataset.animating).toBe("true");
  await render(false);
  expect(layer.dataset.animating).toBe("false");
  expect(layer.querySelector(".runtime-dock-ring__activity")).toBeNull();
});
