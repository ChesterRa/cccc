// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { createTaskColumnPages } from "../../../features/contextModal/taskBoardData";
import type { ContextTranslator } from "../model";
import { TaskColumnJump } from "./TaskColumnJump";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const tr: ContextTranslator = (_key, fallback) => fallback;
function Fixture({
  archived = false,
  scrollMarker = true,
}: {
  archived?: boolean;
  scrollMarker?: boolean;
}) {
  const boardRef = useRef<HTMLElement>(null);
  const pages = createTaskColumnPages();
  pages.active.totalCount = 31;
  pages.planned.totalCount = 2;
  pages.done.loading = true;
  pages.archived.totalCount = 7;
  return (
    <div data-context-scroll={scrollMarker ? "" : undefined}>
      <div data-taskboard-sticky="">
        <div data-taskboard-jump-layer="">
          <TaskColumnJump
            boardRef={boardRef}
            columnPages={pages}
            archivedExpanded={archived}
            tr={tr}
          />
        </div>
      </div>
      <section ref={boardRef}>
        {["active", "planned", "done", ...(archived ? ["archived"] : [])].map((status) => (
          <section key={status} data-task-column={status} tabIndex={-1} />
        ))}
      </section>
    </div>
  );
}

it("shows authoritative loaded counts and only offers the expanded archive", async () => {
  await act(async () => root.render(<Fixture />));
  expect(
    [...host.querySelectorAll("[data-taskboard-jump-column]")].map((node) => node.textContent),
  ).toEqual(["Active 31", "Planned 2", "Done …"]);
  await act(async () => root.render(<Fixture archived />));
  expect(host.querySelector('[data-taskboard-jump-column="archived"]')?.textContent).toBe(
    "Archived 7",
  );
});

it("scrolls the marked modal under its sticky controls and focuses the target", async () => {
  await act(async () => root.render(<Fixture />));
  const scroll = host.querySelector<HTMLElement>("[data-context-scroll]")!;
  const sticky = host.querySelector<HTMLElement>("[data-taskboard-jump-layer]")!;
  const planned = host.querySelector<HTMLElement>('[data-task-column="planned"]')!;
  const rect = (top: number, height: number) => ({ top, bottom: top + height, height }) as DOMRect;
  vi.spyOn(scroll, "getBoundingClientRect").mockReturnValue(rect(40, 500));
  vi.spyOn(sticky, "getBoundingClientRect").mockReturnValue(rect(40, 80));
  vi.spyOn(planned, "getBoundingClientRect").mockReturnValue(rect(720, 200));
  scroll.scrollTop = 200;
  const scrollTo = vi.fn();
  scroll.scrollTo = scrollTo;
  const focus = vi.spyOn(planned, "focus");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[data-taskboard-jump-column="planned"]')!.click(),
  );
  expect(scrollTo).toHaveBeenCalledWith({ top: 792, behavior: "auto" });
  expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(document.activeElement).toBe(planned);
});

it("does not scroll or move focus without its marked container", async () => {
  await act(async () => root.render(<Fixture scrollMarker={false} />));
  const planned = host.querySelector<HTMLElement>('[data-task-column="planned"]')!;
  const focus = vi.spyOn(planned, "focus");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[data-taskboard-jump-column="planned"]')!.click(),
  );
  expect(focus).not.toHaveBeenCalled();
});

it("does not jump if its sticky layer has not been mounted", async () => {
  await act(async () => root.render(<Fixture />));
  host.querySelector("[data-taskboard-jump-layer]")!.removeAttribute("data-taskboard-jump-layer");
  const planned = host.querySelector<HTMLElement>('[data-task-column="planned"]')!;
  const focus = vi.spyOn(planned, "focus");
  await act(async () =>
    host.querySelector<HTMLButtonElement>('[data-taskboard-jump-column="planned"]')!.click(),
  );
  expect(focus).not.toHaveBeenCalled();
});

it("indicates additional columns until the horizontal scroller reaches the end", async () => {
  await act(async () => root.render(<Fixture archived />));
  const nav = host.querySelector<HTMLElement>("[data-taskboard-jump]")!;
  Object.defineProperties(nav, {
    scrollWidth: { configurable: true, value: 400 },
    clientWidth: { configurable: true, value: 250 },
  });
  await act(async () => nav.dispatchEvent(new Event("scroll")));
  expect(host.querySelector("[data-taskboard-jump-more]")).not.toBeNull();
  nav.scrollLeft = 150;
  await act(async () => nav.dispatchEvent(new Event("scroll")));
  expect(host.querySelector("[data-taskboard-jump-more]")).toBeNull();
});
