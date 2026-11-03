import { expect, it } from "vite-plus/test";
import { buildHeadlessRawTraceEntries } from "./headlessRawTimeline";

it("shows completed tool calls as ended without claiming success", () => {
  const entries = buildHeadlessRawTraceEntries([
    {
      id: "started",
      type: "headless.activity.started",
      data: { activity_id: "working", kind: "tool", status: "started", summary: "bash" },
    },
    {
      id: "failed-started",
      type: "headless.activity.started",
      data: { activity_id: "failed", kind: "tool", status: "started", summary: "bash" },
    },
    {
      id: "failed",
      type: "headless.activity.completed",
      data: {
        activity_id: "failed",
        kind: "tool",
        status: "completed",
        summary: "bash failed",
        command: "cargo test",
        tool_name: "bash",
      },
    },
    {
      id: "succeeded",
      type: "headless.activity.completed",
      data: { activity_id: "succeeded", kind: "tool", status: "completed", summary: "bash" },
    },
    { id: "turn", type: "headless.turn.completed", data: { turn_id: "turn-1" } },
  ]);

  expect(entries).toHaveLength(4);
  expect(entries[0]).toMatchObject({ tone: "info", live: true });
  expect(entries[1]).toMatchObject({
    title: "cargo test",
    detailLines: ["bash failed", "tool: bash"],
    tone: "neutral",
    live: false,
  });
  expect(entries[2]).toMatchObject({ tone: "neutral", live: false });
  expect(entries[3]).toMatchObject({ tone: "success", live: false });
});
