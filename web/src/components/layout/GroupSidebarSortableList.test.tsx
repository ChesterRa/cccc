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
          onMenuAction={onMenuAction}
          onReorderSection={vi.fn()}
          onSelectGroup={onSelectGroup}
          onClose={vi.fn()}
        />,
      ),
    );

    // Rows carry no drag handle any more, so every button in the list is an
    // action-menu trigger — one per group, and nothing else.
    expect(host.querySelectorAll("button")).toHaveLength(groups.length);

    const actions = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]');
    await act(async () => actions?.click());
    const archive = document.body.querySelector<HTMLButtonElement>('[role="menuitem"]');
    expect(archive?.textContent).toBe("Archive");
    expect(host.contains(archive)).toBe(false);

    await act(async () => archive?.click());
    expect(onMenuAction).toHaveBeenCalledWith("g_alpha");
    expect(onSelectGroup).not.toHaveBeenCalled();
  });

  it("lists run actions first and fires them for that row's Group only", async () => {
    const onPause = vi.fn();
    const onDelete = vi.fn();
    const onSelectGroup = vi.fn();
    const runActionsFor = vi.fn((group: GroupMeta) =>
      group.group_id === "g_alpha"
        ? [
            { label: "Pause Delivery", onClick: onPause },
            { label: "Stop All Agents", onClick: vi.fn() },
          ]
        : [],
    );
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
          onMenuAction={vi.fn()}
          runActionsFor={runActionsFor}
          trailingActionsFor={() => [{ label: "Delete", onClick: onDelete }]}
          onReorderSection={vi.fn()}
          onSelectGroup={onSelectGroup}
          onClose={vi.fn()}
        />,
      ),
    );

    const alpha = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]');
    await act(async () => alpha?.click());
    const items = document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    expect(Array.from(items, (item) => item.textContent)).toEqual([
      "Pause Delivery",
      "Stop All Agents",
      "Archive",
      "Delete",
    ]);
    await act(async () => items[0]?.click());
    expect(onPause).toHaveBeenCalledOnce();
    expect(onSelectGroup).not.toHaveBeenCalled();

    const beta = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Beta"]');
    await act(async () => beta?.click());
    const betaItems = document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    expect(Array.from(betaItems, (item) => item.textContent)).toEqual(["Archive", "Delete"]);
    await act(async () => betaItems[1]?.click());
    expect(onDelete).toHaveBeenCalledOnce();
  });

  it("opens connections for an unselected Group and preserves its selection", async () => {
    const onOpenConnections = vi.fn();
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
          onMenuAction={vi.fn()}
          connectionsLabel="Group connections"
          onOpenConnections={onOpenConnections}
          onReorderSection={vi.fn()}
          onSelectGroup={onSelectGroup}
          onClose={vi.fn()}
        />,
      ),
    );
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Actions · Alpha"]')!;
    await act(async () => trigger.click());
    const items = document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]');
    expect(Array.from(items, (item) => item.textContent)).toEqual(["Group connections", "Archive"]);
    await act(async () => items[0].click());
    expect(onOpenConnections).toHaveBeenCalledWith("g_alpha");
    expect(onSelectGroup).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(trigger);
  });

  it("reorders within the section from the keyboard and clamps at the edges", async () => {
    const onReorderSection = vi.fn();
    await act(async () =>
      root.render(
        <GroupSidebarSortableList
          groups={groups}
          section="working"
          selectedGroupId="g_alpha"
          isDark
          isCollapsed={false}
          reorderInstructions="Press Alt+Up or Alt+Down to move this group."
          onReorderSection={onReorderSection}
          onSelectGroup={vi.fn()}
          onClose={vi.fn()}
        />,
      ),
    );

    const rows = host.querySelectorAll<HTMLElement>('[role="button"][aria-keyshortcuts]');
    expect(rows).toHaveLength(groups.length);
    const press = (row: HTMLElement, key: string) =>
      act(async () => {
        row.dispatchEvent(
          new KeyboardEvent("keydown", { key, altKey: true, bubbles: true, cancelable: true }),
        );
      });

    await press(rows[0], "ArrowDown");
    expect(onReorderSection).toHaveBeenCalledWith("working", 0, 1);
    await press(rows[0], "ArrowUp");
    await press(rows[1], "ArrowDown");
    // The first row cannot move up and the last cannot move down.
    expect(onReorderSection).toHaveBeenCalledTimes(1);
    // dnd-kit's default "press space to pick up" hint would be wrong here.
    expect(document.body.textContent).toContain("Press Alt+Up or Alt+Down");
  });
});
