import { describe, expect, it } from "vite-plus/test";
import { getTaskMessageRefs, getTaskRefChipLabel } from "../../src/utils/taskRefs";

describe("taskRefs", () => {
  it("filters structured task refs and builds compact labels", () => {
    const refs = getTaskMessageRefs([
      { kind: "task_ref", task_id: "T001", title: "Review routing" },
      { kind: "presentation_ref", slot_id: "slot-1" },
      { kind: "task_ref", task_id: "" },
    ]);

    expect(refs).toHaveLength(1);
    expect(getTaskRefChipLabel(refs[0])).toBe("T001 · Review routing");
  });
});
