// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";

const dnd = vi.hoisted(() => {
  class MouseSensor {}
  class TouchSensor {}
  class KeyboardSensor {}
  return {
    MouseSensor,
    TouchSensor,
    KeyboardSensor,
    useSensor: vi.fn((sensor, options) => ({ sensor, options })),
    useSensors: vi.fn((...sensors) => sensors),
  };
});

vi.mock("@dnd-kit/core", () => ({
  DndContext: ({ children }: { children: React.ReactNode }) => children,
  closestCenter: vi.fn(),
  DragEndEvent: class {},
  KeyboardSensor: dnd.KeyboardSensor,
  MouseSensor: dnd.MouseSensor,
  TouchSensor: dnd.TouchSensor,
  useSensor: dnd.useSensor,
  useSensors: dnd.useSensors,
}));

vi.mock("@dnd-kit/sortable", () => ({
  SortableContext: ({ children }: { children: React.ReactNode }) => children,
  sortableKeyboardCoordinates: vi.fn(),
  verticalListSortingStrategy: vi.fn(),
}));

vi.mock("./SortableGroupItem", () => ({ SortableGroupItem: () => null }));

import { GroupSidebarSortableList } from "./GroupSidebarSortableList";

describe("GroupSidebarSortableList sensors", () => {
  it("registers mouse and touch sensors and no pointer or keyboard sensor", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    dnd.useSensor.mockClear();

    await act(async () => {
      root.render(
        <GroupSidebarSortableList
          groups={[{ group_id: "g_alpha", title: "Alpha" } as GroupMeta]}
          section="working"
          selectedGroupId=""
          isDark={false}
          isCollapsed={false}
          onReorderSection={vi.fn()}
          onSelectGroup={vi.fn()}
          onClose={vi.fn()}
        />,
      );
    });

    expect(dnd.useSensor).toHaveBeenCalledWith(dnd.MouseSensor, {
      activationConstraint: { distance: 4 },
    });
    // A pointer sensor would also receive touch input and turn the first
    // pixels of a scroll into a drag, defeating the long press.
    expect(dnd.useSensor).not.toHaveBeenCalledWith(
      expect.objectContaining({ name: "PointerSensor" }),
      expect.anything(),
    );
    expect(dnd.useSensor).toHaveBeenCalledWith(dnd.TouchSensor, {
      activationConstraint: { delay: 250, tolerance: 8 },
    });
    // The row swallows Enter/Space for selection, so a keyboard sensor could
    // never pick anything up; Alt+Arrow on the row replaces it.
    expect(dnd.useSensor).not.toHaveBeenCalledWith(dnd.KeyboardSensor, expect.anything());

    await act(async () => root.unmount());
  });
});
