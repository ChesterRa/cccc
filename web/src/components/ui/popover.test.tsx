// @vitest-environment happy-dom
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function open(content: ReactElement, scoped = false) {
  await act(async () => {
    root.render(
      <div data-lcd-scope={scoped || undefined}>
        <Popover>
          <PopoverTrigger>Open</PopoverTrigger>
          {content}
        </Popover>
      </div>,
    );
  });
  await act(async () => host.querySelector("button")!.click());
  return document.querySelector<HTMLElement>("[data-popover-probe]")!;
}

it.each([true, false])(
  "takes the trigger's LCD scope across the portal (scoped: %s)",
  async (scoped) => {
    const popover = await open(<PopoverContent data-popover-probe />, scoped);
    expect(host.contains(popover)).toBe(false);
    expect(popover.hasAttribute("data-lcd-scope")).toBe(scoped);
  },
);

it("rechecks the scope each time it opens", async () => {
  const popover = await open(<PopoverContent data-popover-probe />);
  expect(popover.hasAttribute("data-lcd-scope")).toBe(false);
  const trigger = host.querySelector("button")!;
  await act(async () => trigger.click());
  host.querySelector("div")!.setAttribute("data-lcd-scope", "");
  await act(async () => trigger.click());
  expect(document.querySelector("[data-popover-probe]")!.hasAttribute("data-lcd-scope")).toBe(true);
});

it("leaves the scope to a caller that sets it", async () => {
  const popover = await open(<PopoverContent data-popover-probe data-lcd-scope="" />);
  expect(popover.hasAttribute("data-lcd-scope")).toBe(true);
});

it("scrolls within the viewport by default and keeps the caller's limits", async () => {
  const popover = await open(<PopoverContent data-popover-probe style={{ width: "10rem" }} />);
  expect(popover.style.overflowY).toBe("auto");
  expect(popover.style.width).toBe("10rem");
  expect(popover.className).toContain("max-h-[var(--radix-popover-content-available-height)]");

  await act(async () => host.querySelector("button")!.click());
  const capped = await open(<PopoverContent data-popover-probe className="max-h-40" />);
  expect(capped.className).toContain("max-h-40");
  expect(capped.className).not.toContain("max-h-[var(--radix-popover-content-available-height)]");
});
