import { describe, expect, it } from "vite-plus/test";
import { taskToDraft } from "../../src/components/ContextModal/model";
import { normalizeContext } from "../../src/services/api/base";

describe("context summary task contract", () => {
  it("keeps every field that the task editor can write back", () => {
    const context = normalizeContext({
      coordination: {
        tasks: [
          {
            id: "task-1",
            title: "Keep details",
            outcome: "Existing outcome",
            notes: "Existing notes",
            checklist: [{ id: "check-1", text: "Keep this item", status: "in_progress" }],
          },
        ],
      },
    });
    const task = context.coordination?.tasks?.[0];

    expect(task).toBeDefined();
    expect(taskToDraft(task!)).toMatchObject({
      outcome: "Existing outcome",
      notes: "Existing notes",
      checklist: "[~] Keep this item",
    });
  });
});
