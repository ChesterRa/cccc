// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebarItem } from "./GroupSidebarItem";

describe("GroupSidebarItem mobile actions", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the menu visible and isolates its action from group selection", async () => {
    const onMenuAction = vi.fn();
    const onSelect = vi.fn();
    await act(async () =>
      root.render(
        <GroupSidebarItem
          group={{ group_id: "g_alpha", title: "Alpha" } as GroupMeta}
          isActive
          isCollapsed={false}
          menuActionLabel="Archive"
          menuAriaLabel="Actions · Alpha"
          onMenuAction={onMenuAction}
          onSelect={onSelect}
        />,
      ),
    );

    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]');
    expect(trigger).not.toBeNull();
    expect(trigger?.className).toContain("opacity-100");
    expect(trigger?.className).toContain("group-hover/item:opacity-100");
    expect(trigger?.className).toContain("focus-visible:opacity-100");
    expect(trigger?.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[aria-label^="Reorder"]')).toBeNull();

    await act(async () => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    const action = host.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(action?.textContent).toBe("Archive");

    await act(async () => action?.click());
    expect(onMenuAction).toHaveBeenCalledOnce();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("marks an inactive menu trigger for coarse-pointer visibility", async () => {
    await act(async () =>
      root.render(
        <GroupSidebarItem
          group={{ group_id: "g_beta", title: "Beta" } as GroupMeta}
          isActive={false}
          isCollapsed={false}
          menuActionLabel="Archive"
          menuAriaLabel="Actions · Beta"
          onMenuAction={vi.fn()}
          onSelect={vi.fn()}
        />,
      ),
    );

    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Beta"]');
    expect(trigger?.className).toContain("pointer-events-none");
    // `pointer-coarse:` is a built-in Tailwind v4.1+ variant (`@media (pointer: coarse)`).
    expect(trigger?.className).toContain("pointer-coarse:opacity-100");
    expect(trigger?.className).toContain("pointer-coarse:pointer-events-auto");
    expect(trigger?.className).toContain("group-hover/item:pointer-events-auto");
    expect(trigger?.className).toContain("focus-visible:pointer-events-auto");
  });
});
