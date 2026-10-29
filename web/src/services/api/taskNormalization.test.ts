import { describe, expect, it } from "vite-plus/test";

import { normalizeTaskTree } from "./taskNormalization";

describe("normalizeTaskTree", () => {
  it("preserves and recursively normalizes exact-task children", () => {
    const task = normalizeTaskTree({
      id: "T001",
      title: "parent",
      children: [
        {
          id: "T002",
          title: "child",
          children: [{ id: "T003", title: "grandchild" }, { title: "invalid" }],
        },
      ],
    });

    expect(task?.children?.[0]?.id).toBe("T002");
    expect(task?.children?.[0]?.children).toEqual([
      expect.objectContaining({ id: "T003", title: "grandchild" }),
    ]);
  });

  it("keeps each task's safe revision, including the legacy zero revision", () => {
    const task = normalizeTaskTree({
      id: "T001",
      rev: 0,
      children: [
        { id: "T002", rev: 7 },
        { id: "T003", rev: Number.MAX_SAFE_INTEGER },
      ],
    });

    expect(task?.rev).toBe(0);
    expect(task?.children?.map((child) => child.rev)).toEqual([7, Number.MAX_SAFE_INTEGER]);
  });

  it.each([undefined, null, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, "2"])(
    "does not treat an absent or invalid revision %s as a write guard",
    (rev) => {
      expect(normalizeTaskTree({ id: "T001", rev })?.rev).toBeUndefined();
    },
  );

  it("keeps checklist text and custom status intact while dropping blank items", () => {
    const task = normalizeTaskTree({
      id: "T001",
      checklist: [
        { id: "a", text: "  Keep spaces  ", status: "blocked" },
        { id: "b", text: "   ", status: "pending" },
      ],
    });

    expect(task?.checklist).toEqual([{ id: "a", text: "  Keep spaces  ", status: "blocked" }]);
  });

  it("keeps existing checklist IDs and omitted statuses as stored", () => {
    const task = normalizeTaskTree({
      id: "T001",
      checklist: [
        { id: " item-1 ", text: "Spaces are part of this ID" },
        { id: "item-1", text: "A different item", status: "done" },
        { text: "A missing ID" },
      ],
    });

    expect(task?.checklist?.map((item) => item.id)).toEqual([" item-1 ", "item-1", "item-2"]);
    expect(task?.checklist?.[0]?.status).toBeUndefined();
    expect(task?.checklist?.[2]?.status).toBeUndefined();
  });

  it("assigns missing checklist IDs without colliding with explicit IDs later in the list", () => {
    const task = normalizeTaskTree({
      id: "T001",
      checklist: [{ text: "First" }, { id: "item-1", text: "Second" }, { text: "Third" }],
    });

    expect(task?.checklist?.map((item) => item.id)).toEqual(["item-2", "item-1", "item-3"]);
  });
});
