import { describe, expect, it } from "vite-plus/test";
import type { RuntimeActivityEvent } from "../../types";
import { projectRuntimeActivities } from "./runtimeActivityProjection";

const event: RuntimeActivityEvent = {
  v: 1,
  id: "event",
  ts: "2026-07-28T00:00:00Z",
  group_id: "g1",
  actor_id: "peer",
  runtime: "codex",
  activity_id: "tool:1",
  kind: "tool",
  status: "completed",
  event_type: "PostToolUse",
  session_id: "session",
  tool_name: "Bash",
  duration_ms: 1_600,
};

describe("runtime activity projection", () => {
  it("uses structured fields without exposing tool input", () => {
    const t = (key: string, options?: Record<string, unknown>) =>
      `${key}:${String(options?.tool)}:${String(options?.seconds)}`;
    const projected = projectRuntimeActivities([event], t);
    expect(projected[0]?.summary).toBe("runtimeActivity.toolCompleted:Bash:2");
    expect(projected[0]).not.toHaveProperty("command");
  });

  it("shows changing tool names and suppresses generic session or turn copy", () => {
    const t = (_key: string, options?: Record<string, unknown>) =>
      `${String(options?.tool)}:${String(options?.seconds)}`;
    const projected = projectRuntimeActivities(
      [
        { ...event, id: "session", activity_id: "session", kind: "session", tool_name: null },
        { ...event, id: "turn", activity_id: "turn", kind: "turn", tool_name: null },
        { ...event, id: "read", activity_id: "read", status: "started", tool_name: "Read" },
        { ...event, id: "bash", activity_id: "bash", status: "started", tool_name: "Bash" },
      ],
      t,
    );

    expect(projected.map((activity) => activity.summary)).toEqual(["Read:2", "Bash:2"]);
  });
});
