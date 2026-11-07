// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { MessageContent } from "./MessageContent";
import type { MessagePathTarget } from "./messagePathContext";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../LazyMarkdownRenderer", () => ({
  LazyMarkdownRenderer: (props: { content: string }) => (
    <a href={props.content} data-markdown-link>
      link
    </a>
  ),
}));
vi.mock("../viewer/GraphicViewer", () => ({
  GraphicViewer: (props: { src: string }) => <div data-viewer={props.src} />,
}));

let root: Root | null = null;

async function render(text: string, paths?: MessagePathTarget, markdown = false) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <MessageContent
        fallbackText={text}
        shouldRenderMarkdown={markdown}
        isDark={false}
        mentionTokens={[]}
        paths={paths}
      />,
    ),
  );
  return host;
}

/** Whether the page took the click. The test then stops the DOM from following the link. */
async function click(target: Element, init: MouseEventInit = {}) {
  let taken: boolean | null = null;
  const hold = (event: Event) => {
    taken = event.defaultPrevented;
    event.preventDefault();
  };
  document.addEventListener("click", hold);
  const event = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...init });
  await act(async () => void target.dispatchEvent(event));
  document.removeEventListener("click", hold);
  return taken ?? event.defaultPrevented;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

it("opens a media link from a plain message in the page", async () => {
  const host = await render("看这张图 https://example.test/photo.png，好看吗");

  expect(await click(host.querySelector("a")!)).toBe(true);
  expect(document.querySelector('[role="dialog"] [data-viewer]')?.getAttribute("data-viewer")).toBe(
    "https://example.test/photo.png",
  );
});

it("offers the original in a new tab from the preview", async () => {
  const host = await render("https://example.test/photo.png");

  await click(host.querySelector("a")!);

  const open = document.querySelector('[role="dialog"] a[target="_blank"]');
  expect(open?.getAttribute("href")).toBe("https://example.test/photo.png");
});

it.each([{ ctrlKey: true }, { metaKey: true }, { shiftKey: true }, { button: 1 }])(
  "leaves a modified click to the browser: %j",
  async (init) => {
    const host = await render("https://example.test/photo.png");

    expect(await click(host.querySelector("a")!, init)).toBe(false);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  },
);

it("opens a path in the workspace it belongs to", async () => {
  const open = vi.fn();
  const host = await render("看 docs/guide.md:12", { scopeKey: "s1", rootPath: "/repo", open });

  await click(host.querySelector("[data-message-path]")!);

  expect(open).toHaveBeenCalledWith("docs/guide.md");
});

it("opens a Markdown link to a workspace file instead of leaving CCCC", async () => {
  const open = vi.fn();
  const paths = { scopeKey: "s1", rootPath: "/repo", open };
  const host = await render("docs/guide.md", paths, true);

  expect(await click(host.querySelector("[data-markdown-link]")!)).toBe(true);

  expect(open).toHaveBeenCalledWith("docs/guide.md");
});

it("leaves a modified click on a file link to the browser", async () => {
  const open = vi.fn();
  const paths = { scopeKey: "s1", rootPath: "/repo", open };
  const host = await render("docs/guide.md", paths, true);

  expect(await click(host.querySelector("[data-markdown-link]")!, { ctrlKey: true })).toBe(false);

  expect(open).not.toHaveBeenCalled();
});

it("opens a path from the keyboard", async () => {
  const open = vi.fn();
  const host = await render("看 docs/guide.md", { scopeKey: "s1", rootPath: "/repo", open });
  const path = host.querySelector("[data-message-path]")!;

  await act(
    async () =>
      void path.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true })),
  );
  expect(open).not.toHaveBeenCalled();

  await act(
    async () =>
      void path.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })),
  );
  expect(open).toHaveBeenCalledWith("docs/guide.md");
});

it("shows a path as text where nothing can open it", async () => {
  const host = await render("看 docs/guide.md:12");

  expect(host.querySelector("[data-message-path]")).toBeNull();
});

it("leaves a web page link to the browser", async () => {
  const host = await render("https://example.test/page");

  expect(await click(host.querySelector("a")!)).toBe(false);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});
