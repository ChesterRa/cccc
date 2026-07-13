import { describe, expect, it } from "vite-plus/test";

import {
  buildHeadlessPreviewRenderGroups,
  buildHeadlessPreviewTimelineEntries,
} from "../../../src/pages/chat/headlessPreviewTimeline";

describe("buildHeadlessPreviewTimelineEntries", () => {
  it("merges message and activity rows into one chronological stream across recent sessions", () => {
    const entries = buildHeadlessPreviewTimelineEntries({
      previewSessions: [
        {
          actorId: "coder",
          pendingEventId: "evt-1",
          currentStreamId: "commentary-1",
          phase: "completed",
          streamPhase: "final_answer",
          updatedAt: "2025-01-01T00:00:03Z",
          latestText: "Done",
          transcriptBlocks: [
            {
              id: "commentary-1::commentary",
              streamId: "commentary-1",
              streamPhase: "commentary",
              text: "Inspecting files",
              updatedAt: "2025-01-01T00:00:01Z",
              completed: true,
              transient: true,
            },
            {
              id: "final-1::final_answer",
              streamId: "final-1",
              streamPhase: "final_answer",
              text: "Done",
              updatedAt: "2025-01-01T00:00:03Z",
              completed: true,
              transient: false,
            },
          ],
          activities: [
            {
              id: "tool-1",
              kind: "tool",
              status: "completed",
              summary: "rg -n reducer",
              ts: "2025-01-01T00:00:02Z",
            },
          ],
        },
        {
          actorId: "coder",
          pendingEventId: "evt-2",
          currentStreamId: "commentary-2",
          phase: "streaming",
          streamPhase: "commentary",
          updatedAt: "2025-01-01T00:00:05Z",
          latestText: "Writing fix",
          transcriptBlocks: [
            {
              id: "commentary-2::commentary",
              streamId: "commentary-2",
              streamPhase: "commentary",
              text: "Writing fix",
              updatedAt: "2025-01-01T00:00:05Z",
              completed: false,
              transient: true,
            },
          ],
          activities: [
            {
              id: "thinking-2",
              kind: "thinking",
              status: "started",
              summary: "Planning patch",
              ts: "2025-01-01T00:00:04Z",
            },
          ],
        },
      ],
    });

    expect(entries.map((entry) => entry.kind)).toEqual([
      "message",
      "activity",
      "message",
      "activity",
      "message",
    ]);
    expect(entries.map((entry) => entry.pendingEventId)).toEqual([
      "evt-1",
      "evt-1",
      "evt-1",
      "evt-2",
      "evt-2",
    ]);
    expect(entries[3]).toMatchObject({ kind: "activity", live: true, pendingEventId: "evt-2" });
    expect(entries[4]).toMatchObject({ kind: "message", live: true, pendingEventId: "evt-2" });
  });

  it("falls back to synthetic entries when only raw fallback text and activities exist", () => {
    const entries = buildHeadlessPreviewTimelineEntries({
      fallbackText: "Fallback output",
      fallbackActivities: [
        {
          id: "activity-1",
          kind: "tool",
          status: "started",
          summary: "search docs",
          ts: "2025-01-01T00:00:00Z",
        },
      ],
      fallbackUpdatedAt: "2025-01-01T00:00:01Z",
      fallbackPendingEventId: "fallback-1",
      fallbackStreamId: "stream-fallback",
      fallbackStreamPhase: "final_answer",
    });

    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ kind: "activity", pendingEventId: "fallback-1" });
    expect(entries[1]).toMatchObject({
      kind: "message",
      pendingEventId: "fallback-1",
      streamPhase: "final_answer",
    });
  });

  it("keeps fallback entries non-live when the provided fallback phase is completed", () => {
    const entries = buildHeadlessPreviewTimelineEntries({
      fallbackText: "Reply complete",
      fallbackActivities: [
        {
          id: "activity-1",
          kind: "tool",
          status: "completed",
          summary: "search docs",
          ts: "2025-01-01T00:00:00Z",
        },
      ],
      fallbackUpdatedAt: "2025-01-01T00:00:01Z",
      fallbackPendingEventId: "fallback-1",
      fallbackStreamId: "stream-fallback",
      fallbackStreamPhase: "final_answer",
      fallbackPhase: "completed",
    });

    expect(entries).toHaveLength(2);
    expect(entries.every((entry) => entry.live === false)).toBe(true);
  });

  it("suppresses reasoning activity rows that duplicate commentary transcript text", () => {
    const entries = buildHeadlessPreviewTimelineEntries({
      previewSessions: [
        {
          actorId: "coder",
          pendingEventId: "evt-1",
          currentStreamId: "commentary-1",
          phase: "streaming",
          streamPhase: "commentary",
          updatedAt: "2025-01-01T00:00:04Z",
          latestText: "I will inspect the runtime output first",
          transcriptBlocks: [
            {
              id: "commentary-1::commentary",
              streamId: "commentary-1",
              streamPhase: "commentary",
              text: "I will inspect the runtime output first",
              updatedAt: "2025-01-01T00:00:04Z",
              completed: false,
              transient: true,
            },
          ],
          activities: [
            {
              id: "reasoning-1",
              kind: "thinking",
              status: "completed",
              summary: "I will inspect the runtime output first",
              raw_item_type: "reasoning",
              ts: "2025-01-01T00:00:03Z",
            },
            {
              id: "tool-1",
              kind: "tool",
              status: "completed",
              summary: "rg -n runtime",
              ts: "2025-01-01T00:00:02Z",
            },
          ],
        },
      ],
    });

    expect(entries.map((entry) => entry.kind)).toEqual(["activity", "message"]);
    expect(entries[0]).toMatchObject({ kind: "activity", activity: { id: "tool-1" } });
    expect(entries[1]).toMatchObject({
      kind: "message",
      text: "I will inspect the runtime output first",
    });
  });

  it("groups contiguous activity rows into compact activity bands", () => {
    const entries = buildHeadlessPreviewTimelineEntries({
      previewSessions: [
        {
          actorId: "coder",
          pendingEventId: "evt-1",
          currentStreamId: "commentary-1",
          phase: "streaming",
          streamPhase: "commentary",
          updatedAt: "2025-01-01T00:00:04Z",
          latestText: "Applying fix",
          transcriptBlocks: [
            {
              id: "commentary-1::commentary",
              streamId: "commentary-1",
              streamPhase: "commentary",
              text: "Applying fix",
              updatedAt: "2025-01-01T00:00:04Z",
              completed: false,
              transient: true,
            },
          ],
          activities: [
            {
              id: "tool-1",
              kind: "tool",
              status: "completed",
              summary: "rg -n reducer",
              ts: "2025-01-01T00:00:01Z",
            },
            {
              id: "patch-1",
              kind: "patch",
              status: "completed",
              summary: "edit reducer.ts",
              ts: "2025-01-01T00:00:02Z",
            },
            {
              id: "plan-1",
              kind: "plan",
              status: "updated",
              summary: "verify fix",
              ts: "2025-01-01T00:00:03Z",
            },
          ],
        },
      ],
    });

    const groups = buildHeadlessPreviewRenderGroups(entries);

    expect(groups.map((group) => group.kind)).toEqual(["activity-band", "message"]);
    expect(groups[0]).toMatchObject({ kind: "activity-band", pendingEventId: "evt-1" });
    expect(
      groups[0]?.kind === "activity-band"
        ? groups[0].entries.map((entry) => entry.activity.id)
        : [],
    ).toEqual(["tool-1", "patch-1", "plan-1"]);
  });

  it("starts a new activity band when the session changes", () => {
    const groups = buildHeadlessPreviewRenderGroups([
      {
        id: "activity:evt-1:tool-1",
        kind: "activity",
        pendingEventId: "evt-1",
        ts: "2025-01-01T00:00:01Z",
        live: false,
        activity: { id: "tool-1", kind: "tool", status: "completed", summary: "scan files" },
      },
      {
        id: "activity:evt-2:tool-2",
        kind: "activity",
        pendingEventId: "evt-2",
        ts: "2025-01-01T00:00:02Z",
        live: true,
        activity: { id: "tool-2", kind: "tool", status: "started", summary: "open patch" },
      },
    ]);

    expect(groups).toHaveLength(2);
    expect(groups.map((group) => group.kind)).toEqual(["activity-band", "activity-band"]);
    expect(groups[0]?.pendingEventId).toBe("evt-1");
    expect(groups[1]?.pendingEventId).toBe("evt-2");
  });
});
