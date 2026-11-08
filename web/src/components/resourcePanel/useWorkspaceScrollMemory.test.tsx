// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import type { WorkspaceFileNavigation } from "../workspace/useWorkspaceEditor";
import { useWorkspaceScrollMemory } from "./useWorkspaceScrollMemory";

let host: HTMLDivElement;
let root: Root;
/** How far the content of the one reused scroller can scroll; tests grow it to mimic late layout. */
let maxScroll = 0;
let forget: (keys: string[]) => void = () => undefined;

function Viewer({
  shownKey,
  anchor = null,
}: {
  shownKey: string;
  anchor?: WorkspaceFileNavigation | null;
}) {
  const body = useRef<HTMLDivElement>(null);
  forget = useWorkspaceScrollMemory(body, shownKey, anchor);
  return (
    <div ref={body}>
      <div data-workspace-scroll ref={clampScroll} />
    </div>
  );
}

/** Happy DOM has no layout, so the scroller clamps like a browser against `maxScroll`. */
function clampScroll(node: HTMLDivElement | null) {
  if (!node || "clamped" in node.dataset) return;
  node.dataset.clamped = "";
  let top = 0;
  Object.defineProperty(node, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = Math.max(0, Math.min(value, maxScroll));
    },
  });
}

const scroller = () => host.querySelector<HTMLElement>("[data-workspace-scroll]")!;
const show = (shownKey: string, anchor: WorkspaceFileNavigation | null = null) =>
  act(async () => root.render(<Viewer shownKey={shownKey} anchor={anchor} />));
/** The reader scrolls: the position changes, then the browser reports it. */
const read = (top: number) => {
  scroller().scrollTop = top;
  scroller().dispatchEvent(new Event("scroll"));
};
const frames = (count = 3) =>
  act(async () => {
    for (let i = 0; i < count; i += 1) await new Promise((done) => requestAnimationFrame(done));
  });

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  maxScroll = 5000;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("brings each tab back to where it was read, and starts a new one at the top", async () => {
  await show("workspace:s1:/repo:a.md");
  read(1200);
  await show("");
  await show("workspace:s1:/repo:b.md");
  // The shared scroller still holds a.md's position; b.md was never read and starts at the top.
  expect(scroller().scrollTop).toBe(0);
  read(300);
  await show("workspace:s1:/repo:a.md");
  expect(scroller().scrollTop).toBe(1200);
  await show("workspace:s1:/repo:b.md");
  expect(scroller().scrollTop).toBe(300);
});

it("keeps the same path in another workspace root apart", async () => {
  await show("workspace:s1:/repo:README.md");
  read(800);
  await show("workspace:s1:/other:README.md");
  expect(scroller().scrollTop).toBe(0);
});

it("does not record while another file is still on screen or loading", async () => {
  await show("workspace:s1:/repo:a.md");
  read(400);
  await show("");
  read(2500);
  await show("workspace:s1:/repo:a.md");
  expect(scroller().scrollTop).toBe(400);
});

it("waits for content that reaches its height later, like a Markdown render", async () => {
  await show("workspace:s1:/repo:long.md");
  read(3000);
  await show("workspace:s1:/repo:short.md");
  maxScroll = 200;
  await show("workspace:s1:/repo:long.md");
  expect(scroller().scrollTop).toBe(200);
  maxScroll = 5000;
  await frames();
  expect(scroller().scrollTop).toBe(3000);
});

it("stops restoring once the reader scrolls during the wait", async () => {
  await show("workspace:s1:/repo:long.md");
  read(3000);
  await show("workspace:s1:/repo:other.md");
  maxScroll = 100;
  await show("workspace:s1:/repo:long.md");
  host.querySelector("div")!.dispatchEvent(new Event("wheel"));
  read(50);
  maxScroll = 5000;
  await frames();
  expect(scroller().scrollTop).toBe(50);
});

it("leaves a file opened at a new anchor where the anchor put it, then remembers from there", async () => {
  await show("workspace:s1:/repo:a.md");
  read(900);
  await show("workspace:s1:/repo:b.md");
  scroller().scrollTop = 40;
  const section = { fragment: "section" };
  await show("workspace:s1:/repo:a.md", section);
  expect(scroller().scrollTop).toBe(40);
  read(1500);
  // Away to an attachment and back: the editor still holds the same anchor, which is not new.
  await show("", section);
  scroller().scrollTop = 0;
  await show("workspace:s1:/repo:a.md", section);
  expect(scroller().scrollTop).toBe(1500);
});

it("forgets a closed tab's position", async () => {
  await show("workspace:s1:/repo:a.md");
  read(700);
  await show("workspace:s1:/repo:b.md");
  forget(["workspace:s1:/repo:a.md"]);
  await show("workspace:s1:/repo:a.md");
  expect(scroller().scrollTop).toBe(0);
});
