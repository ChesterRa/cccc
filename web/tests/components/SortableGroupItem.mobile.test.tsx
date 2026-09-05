// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

const sortableMocks = vi.hoisted(() => ({ onPointerDown: vi.fn(), onTouchStart: vi.fn() }));

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
import type { GroupMeta } from "../../src/types";

describe("SortableGroupItem mobile activation", () => {
  it("starts sorting from the row without rendering a drag handle or leaking menu presses", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    sortableMocks.onPointerDown.mockClear();
    sortableMocks.onTouchStart.mockClear();
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

    // The action menu is the only button left on a row; no drag handle.
    expect(host.querySelectorAll("button")).toHaveLength(1);
    expect(host.querySelector('button[aria-label="Group actions"]')).not.toBeNull();

    const item = host.querySelector<HTMLElement>('[role="button"]')!;
    item.dispatchEvent(new Event("touchstart", { bubbles: true }));
    expect(sortableMocks.onTouchStart).toHaveBeenCalledOnce();

    sortableMocks.onTouchStart.mockClear();
    const menu = host.querySelector<HTMLButtonElement>('button[aria-label="Group actions"]')!;
    menu.dispatchEvent(new Event("touchstart", { bubbles: true }));
    expect(sortableMocks.onTouchStart).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    host.remove();
  });
});
