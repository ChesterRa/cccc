import { expect, it } from "vite-plus/test";
import type { HeadlessPreviewSession, LedgerEvent } from "../../types";
import { projectUnboundTrace } from "./unboundTraceProjection";

const old: HeadlessPreviewSession = {
  actorId: "a",
  pendingEventId: "old-request",
  currentStreamId: "old-stream",
  phase: "completed",
  streamPhase: "",
  updatedAt: "2026-01-01T00:00:00Z",
  latestText: "old",
  activities: [],
  transcriptBlocks: [
    {
      id: "old",
      streamId: "old-stream",
      streamPhase: "",
      text: "old",
      updatedAt: "2026-01-01T00:00:00Z",
      completed: true,
      transient: false,
    },
  ],
};
const row = (
  actor: string,
  group: string,
  id: string,
  ts: string,
  data: Record<string, unknown> = {},
): LedgerEvent => ({
  by: actor,
  group_id: group,
  ts,
  _streaming: true,
  data: { stream_id: id, text: "hello", _headless_text_started_at: ts, ...data },
});

it("projects the latest matching unbound headless row without inventing a pending event", () => {
  const rows = [
    row("a", "g", "earlier", "2026-01-01T00:00:01Z"),
    row("b", "g", "other-actor", "2026-01-01T00:00:09Z"),
    row("a", "elsewhere", "other-group", "2026-01-01T00:00:10Z"),
    row("a", "g", "new", "2026-01-01T00:00:03Z", {
      _headless_text_started_at: "2026-01-01T00:00:02Z",
      activities: [{ id: "tool", kind: "tool", status: "completed", summary: "bash failed" }],
    }),
  ];
  const session = projectUnboundTrace("g", "a", rows, [old])!;
  expect(session.pendingEventId).toBe("");
  expect(session.currentStreamId).toBe("new");
  expect(session.transcriptBlocks[0].updatedAt).toBe("2026-01-01T00:00:02Z");
  expect(session.activities).toMatchObject([{ id: "tool", summary: "bash failed" }]);
});
it("ignores rows without text, headless metadata, stream identity, or with a reply binding", () => {
  const rows = [
    row("a", "g", "empty", "2026-01-01T00:00:01Z", { text: "" }),
    row("a", "g", "ordinary", "2026-01-01T00:00:02Z", { _headless_text_started_at: undefined }),
    row("a", "g", "", "2026-01-01T00:00:03Z"),
    row("a", "g", "pending", "2026-01-01T00:00:04Z", { pending_event_id: "request" }),
    row("a", "g", "reply", "2026-01-01T00:00:05Z", { reply_to: "request" }),
    row("a", "g", "old-stream", "2026-01-01T00:00:06Z"),
  ];
  expect(projectUnboundTrace("g", "a", rows, [old])).toBeNull();
});
