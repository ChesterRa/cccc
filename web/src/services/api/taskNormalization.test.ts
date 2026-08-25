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
});
