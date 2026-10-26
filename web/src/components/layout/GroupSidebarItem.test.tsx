// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebarSectionHeader } from "./GroupSidebarSectionHeader";
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
    expect(trigger?.getAttribute("aria-haspopup")).toBe("menu");
    expect(trigger?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('[role="button"]')?.getAttribute("aria-current")).toBe("page");
    expect(host.querySelector('[aria-label^="Reorder"]')).toBeNull();

    await act(async () => trigger?.click());
    expect(trigger?.getAttribute("aria-expanded")).toBe("true");
    const action = document.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(action?.textContent).toBe("Archive");

    await act(async () => action?.click());
    expect(onMenuAction).toHaveBeenCalledOnce();
    expect(onSelect).not.toHaveBeenCalled();
  });
  it("opens complete instance details from the compact section header", async () => {
    const name = "workstation-with-a-long-name.local";
    await act(async () =>
      root.render(
        <GroupSidebarSectionHeader
          instanceLabel={name}
          instanceOrigin="https://instance.example"
        />,
      ),
    );
    expect(host.textContent).not.toContain(name);
    const trigger = host.querySelector<HTMLButtonElement>("button")!;
    await act(async () => trigger.click());
    const dialog = document.querySelector('[role="dialog"]');
    expect(dialog?.textContent).toContain(name);
    expect(dialog?.textContent).toContain("https://instance.example");
    await act(async () => trigger.click());
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  });
});
