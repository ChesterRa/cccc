import { expect, it } from "vite-plus/test";
import { EMPTY_CHAT_BUCKET } from "./groupStoreCore";
import {
  reconcileStreamingMessagePatch,
  upsertStreamingActivityPatch,
} from "./groupStreamingReducers";

it("keeps the first text timestamp through later tool and text updates", () => {
  let bucket = { ...EMPTY_CHAT_BUCKET };
  const base = {
    streamId: "s",
    pendingEventId: undefined,
    completed: false,
    transientStream: false,
    activities: [],
  };
  bucket = {
    ...bucket,
    ...reconcileStreamingMessagePatch(bucket, "g", "a", {
      ...base,
      ts: "2026-01-01T00:00:01Z",
      firstTextTs: "2026-01-01T00:00:01Z",
      fullText: "A",
      eventText: "A",
    }),
  };
  bucket = {
    ...bucket,
    ...upsertStreamingActivityPatch(
      bucket,
      "g",
      "a",
      { streamId: "s" },
      { id: "t", kind: "tool", status: "completed", summary: "bash", ts: "2026-01-01T00:00:02Z" },
    ),
  };
  bucket = {
    ...bucket,
    ...reconcileStreamingMessagePatch(bucket, "g", "a", {
      ...base,
      ts: "2026-01-01T00:00:03Z",
      firstTextTs: "2026-01-01T00:00:03Z",
      fullText: "AB",
      eventText: "AB",
    }),
  };
  expect(
    (bucket.streamingEvents[0].data as { _headless_text_started_at?: string })
      ._headless_text_started_at,
  ).toBe("2026-01-01T00:00:01Z");
});
