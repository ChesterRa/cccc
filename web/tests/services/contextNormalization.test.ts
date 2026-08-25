import { describe, expect, it } from "vite-plus/test";
import { normalizeContext } from "../../src/services/api/base";

describe("context normalization", () => {
  it("preserves the task-specific revision from an overview", () => {
    const context = normalizeContext({
      version: "ctxv:9",
      tasks_version: "tasksv:4",
      coordination: { brief: { objective: "Ship" } },
      agent_states: [],
    });

    expect(context.version).toBe("ctxv:9");
    expect(context.tasks_version).toBe("tasksv:4");
    expect(context.coordination?.tasks).toBeUndefined();
  });
});
