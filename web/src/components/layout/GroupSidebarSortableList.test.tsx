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

    // Each row has an accessible Group button and a separate menu trigger.
    expect(host.querySelectorAll("button")).toHaveLength(groups.length * 2);
    // Each row starts with the name; the status symbol sits at the row end.
    const groupRows = host.querySelectorAll('[role="button"]');
    expect(groupRows).toHaveLength(groups.length);
    groupRows.forEach((row, index) => {
      expect(row.firstElementChild?.textContent).toBe(groups[index].title);
      expect(row.lastElementChild?.classList.contains("lcd-group-symbol")).toBe(true);
    });

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

  it("keeps every row's columns, and carries the connection count in the menu", async () => {
    const future = new Date(Date.now() + 60_000).toISOString();
    const past = new Date(Date.now() - 60_000).toISOString();
    const render = (summary: { counts: Record<string, number | null>; expires_at: string }) =>
      act(async () =>
        root.render(
          <GroupSidebarSortableList
            groups={[...groups, { group_id: "g_gamma", title: "Gamma" } as GroupMeta]}
            section="working"
            selectedGroupId="g_beta"
            isDark
            isCollapsed={false}
            menuActionLabel="Archive"
            menuAriaLabel="Actions"
            onMenuAction={vi.fn()}
            connectionsLabel="Group connections"
            connectionSummary={summary}
            onOpenConnections={vi.fn()}
            onReorderSection={vi.fn()}
            onSelectGroup={vi.fn()}
            onClose={vi.fn()}
          />,
        ),
      );
    await render({ counts: { g_alpha: 7, g_beta: null }, expires_at: future });
    // No connection control on the rows; each row is its button and a menu slot. Replies
    // waiting for the user are counted beside Chet, not on the rows.
    expect(host.querySelector('[aria-label^="groupConnections."]')).toBeNull();
    const rows = [...host.querySelectorAll('[aria-current], button[role="button"]')];
    expect(rows.map((row) => row.textContent)).toEqual(["Alpha", "Beta", "Gamma"]);
    rows.forEach((row) => expect(row.parentElement?.children).toHaveLength(2));
    expect(host.querySelector('button[aria-label="Actions · Alpha"]')!.className).toContain(
      "pointer-coarse:min-w-[44px]",
    );
    const open = async (title: string) => {
      await act(async () =>
        host.querySelector<HTMLButtonElement>(`button[aria-label="Actions · ${title}"]`)!.click(),
      );
      return document.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
    };
    const alpha = await open("Alpha");
    expect(alpha.textContent).toBe("Group connections · 7");
    expect(alpha.getAttribute("aria-label")).toBe("groupConnections.count");
    alpha.focus();
    // A new count while the menu is open updates the entry without replacing it.
    await render({ counts: { g_alpha: 8, g_beta: null }, expires_at: future });
    expect(document.activeElement).toBe(alpha);
    expect(alpha.textContent).toBe("Group connections · 8");
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    // Unknown and expired counts show "?"; a Group without connections keeps the plain entry.
    expect((await open("Beta")).textContent).toBe("Group connections · ?");
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    await render({ counts: { g_alpha: 7 }, expires_at: past });
    expect((await open("Alpha")).textContent).toBe("Group connections · ?");
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    const gamma = await open("Gamma");
    expect(gamma.textContent).toBe("Group connections");
    expect(gamma.hasAttribute("aria-label")).toBe(false);
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
