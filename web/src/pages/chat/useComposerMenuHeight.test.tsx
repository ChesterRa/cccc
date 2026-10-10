// @vitest-environment happy-dom

import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { VISUAL_VIEWPORT_BOX_ATTRIBUTE } from "../../hooks/useViewportHeight";
import { ChatMentionMenu } from "./ChatMentionMenu";
import { SlashCommandMenu } from "./SlashCommandMenu";

// The composer row sits 400px below the visible top. Tailwind is not loaded, so
// the menu's bottom margin is 0 and only the 8px edge gutter is subtracted.
const ANCHOR_VISIBLE_TOP = 400;
const EXPECTED_MAX_HEIGHT = "min(15rem, 392px)";

type ViewportCase = {
  name: string;
  /** visualViewport.offsetTop while the keyboard pans the page. */
  viewportOffsetTop: number;
  /** Where the browser reports the app box (placed at the visible top) in client coordinates. */
  appBoxClientTop: number;
};

const VIEWPORT_CASES: ViewportCase[] = [
  // Client rects with a visual-viewport origin (seen on iOS WebKit): the panned
  // app box is at 0 even though visualViewport.offsetTop is large.
  { name: "iOS keyboard pan", viewportOffsetTop: 380, appBoxClientTop: 0 },
  // Client rects with a layout-viewport origin: the app box sits at the pan.
  { name: "layout-viewport client rects", viewportOffsetTop: 380, appBoxClientTop: 380 },
  { name: "desktop", viewportOffsetTop: 0, appBoxClientTop: 0 },
];

const MENUS: Array<{ name: string; render: () => ReactNode }> = [
  {
    name: "@ mention menu",
    render: () => (
      <ChatMentionMenu
        isDark
        isSmallScreen
        items={[{ kind: "agent", value: "@foreman", label: "@foreman" }]}
        left={8}
        selectedIndex={0}
        onSelect={() => {}}
        onHover={() => {}}
      />
    ),
  },
  {
    name: "/ slash menu",
    render: () => (
      <SlashCommandMenu
        isDark
        suggestions={[
          {
            name: "review",
            command: "/review",
            capabilityId: "skill:review",
            sourceType: "capsule_skill",
          },
        ]}
        selectedIndex={0}
        loadMoreLabel="more"
        onSelect={() => {}}
        onHover={() => {}}
      />
    ),
  },
];

describe("useComposerMenuHeight", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let appBoxClientTop = 0;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.hasAttribute(VISUAL_VIEWPORT_BOX_ATTRIBUTE))
          return new DOMRect(0, appBoxClientTop);
        if (this.dataset.testAnchor !== undefined) {
          return new DOMRect(0, appBoxClientTop + ANCHOR_VISIBLE_TOP);
        }
        return new DOMRect();
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

  for (const viewportCase of VIEWPORT_CASES) {
    for (const menu of MENUS) {
      it(`keeps the ${menu.name} visible above the composer (${viewportCase.name})`, async () => {
        appBoxClientTop = viewportCase.appBoxClientTop;
        vi.stubGlobal(
          "visualViewport",
          Object.assign(new EventTarget(), { offsetTop: viewportCase.viewportOffsetTop }),
        );

        await act(async () =>
          root.render(
            <div {...{ [VISUAL_VIEWPORT_BOX_ATTRIBUTE]: "" }}>
              <div data-test-anchor="">{menu.render()}</div>
            </div>,
          ),
        );

        const listbox = host.querySelector<HTMLElement>("[role='listbox']");
        expect(listbox?.style.maxHeight).toBe(EXPECTED_MAX_HEIGHT);
      });
    }
  }
});
