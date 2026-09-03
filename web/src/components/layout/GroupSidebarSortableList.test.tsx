// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebarSortableList } from "./GroupSidebarSortableList";

const groups = [
  { group_id: "g_alpha", title: "Alpha", running: true },
  { group_id: "g_beta", title: "Beta", running: false },
] as GroupMeta[];

describe("GroupSidebarSortableList mobile controls", () => {
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

  it("keeps the action menu in a portal and never selects through an archive click", async () => {
    const onMenuAction = vi.fn();
    const onSelectGroup = vi.fn();
    await act(async () =>
      root.render(
        <GroupSidebarSortableList
          groups={groups}
          section="working"
          selectedGroupId="g_beta"
          isDark
          isCollapsed={false}
          menuActionLabel="Archive"
          menuAriaLabel="Actions"
          dragHandleLabel="Reorder"
          onMenuAction={onMenuAction}
          onReorderSection={vi.fn()}
          onSelectGroup={onSelectGroup}
          onClose={vi.fn()}
        />,
      ),
    );

    const handle = host.querySelector<HTMLButtonElement>('button[aria-label="Reorder · Alpha"]');
    expect(handle?.className).toContain("touch-none");
    expect(handle?.style.touchAction).toBe("none");

    const actions = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]');
    await act(async () => actions?.click());
    const archive = document.body.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(archive?.textContent).toBe("Archive");
    expect(host.contains(archive)).toBe(false);

    await act(async () => archive?.click());
    expect(onMenuAction).toHaveBeenCalledWith("g_alpha");
    expect(onSelectGroup).not.toHaveBeenCalled();
  });
});
