// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

const sortableMocks = vi.hoisted(() => ({ onKeyDown: vi.fn(), onPointerDown: vi.fn() }));

vi.mock("@dnd-kit/sortable", () => ({
  useSortable: () => ({
    attributes: { "aria-roledescription": "sortable" },
    listeners: sortableMocks,
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));

import { SortableGroupItem } from "../../src/components/layout/SortableGroupItem";
import { GroupMeta } from "../../src/types";

describe("SortableGroupItem", () => {
  it("drags from the row itself and renders no drag handle at all", async () => {
    sortableMocks.onPointerDown.mockClear();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          onSelect={vi.fn()}
        />,
      );
    });

    // The six-dot grip is gone on every viewport: the only button a row may
    // still carry is the action menu, which this render does not request.
    expect(host.querySelector("button")).toBeNull();

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    item.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));

    expect(sortableMocks.onPointerDown).toHaveBeenCalledOnce();
    expect(item.className).toContain("cursor-grab");
    expect(item.getAttribute("aria-roledescription")).toBe("sortable");

    await act(async () => root.unmount());
    host.remove();
  });

  it("drops the grab cursor when reordering is disabled", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          dragDisabled
          onSelect={vi.fn()}
        />,
      );
    });

    expect(host.querySelector<HTMLElement>('[role="button"]')!.className).not.toContain(
      "cursor-grab",
    );

    await act(async () => root.unmount());
    host.remove();
  });

  it("opens the group action from the native context menu", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const onMenuAction = vi.fn();

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          menuActionLabel="Archive group"
          onMenuAction={onMenuAction}
          onSelect={vi.fn()}
        />,
      );
    });

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    await act(async () => {
      item.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: 40,
          clientY: 60,
        }),
      );
    });

    const action = Array.from(document.body.querySelectorAll("button")).find(
      (button) => button.textContent === "Archive group",
    );
    expect(action).toBeDefined();
    await act(async () => action?.click());
    expect(onMenuAction).toHaveBeenCalledOnce();

    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the group action available from the desktop keyboard menu shortcut", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          menuActionLabel="Archive group"
          onMenuAction={vi.fn()}
          onSelect={vi.fn()}
        />,
      );
    });

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    await act(async () => {
      item.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "F10",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    expect(
      Array.from(document.body.querySelectorAll("button")).some(
        (button) => button.textContent === "Archive group",
      ),
    ).toBe(true);

    await act(async () => root.unmount());
    host.remove();
  });

  it("moves the group with Alt+Arrow from the keyboard without selecting it", async () => {
    const onMoveBy = vi.fn();
    const onSelect = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          onMoveBy={onMoveBy}
          onSelect={onSelect}
        />,
      );
    });

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    expect(item.getAttribute("aria-keyshortcuts")).toBe("Alt+ArrowUp Alt+ArrowDown");
    const press = (key: string, altKey: boolean) =>
      act(async () => {
        item.dispatchEvent(
          new KeyboardEvent("keydown", { key, altKey, bubbles: true, cancelable: true }),
        );
      });

    await press("ArrowDown", true);
    expect(onMoveBy).toHaveBeenLastCalledWith(1);
    await press("ArrowUp", true);
    expect(onMoveBy).toHaveBeenLastCalledWith(-1);
    // A bare arrow is left to the browser, and nothing here selects the group.
    await press("ArrowDown", false);
    expect(onMoveBy).toHaveBeenCalledTimes(2);
    expect(onSelect).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    host.remove();
  });

  it("offers no keyboard reorder while reordering is disabled", async () => {
    const onMoveBy = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          dragDisabled
          onMoveBy={onMoveBy}
          onSelect={vi.fn()}
        />,
      );
    });

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    expect(item.hasAttribute("aria-keyshortcuts")).toBe(false);
    await act(async () => {
      item.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowDown",
          altKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(onMoveBy).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps the touch action reachable without starting a drag", async () => {
    sortableMocks.onPointerDown.mockClear();
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <SortableGroupItem
          group={{ group_id: "g_aquant", title: "AQuant" } as GroupMeta}
          isActive
          isDark={false}
          isCollapsed={false}
          menuActionLabel="Archive group"
          menuAriaLabel="Group actions"
          onMenuAction={vi.fn()}
          onSelect={vi.fn()}
        />,
      );
    });

    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Group actions"]')!;
    expect(trigger).toBeDefined();
    expect(trigger.className).toContain("group-hover/item:opacity-100");
    expect(trigger.className).toContain("focus-visible:opacity-100");
    await act(async () => {
      trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
      trigger.click();
    });

    expect(sortableMocks.onPointerDown).not.toHaveBeenCalled();
    expect(
      Array.from(document.body.querySelectorAll("button")).some(
        (button) => button.textContent === "Archive group",
      ),
    ).toBe(true);

    await act(async () => root.unmount());
    host.remove();
  });
});
