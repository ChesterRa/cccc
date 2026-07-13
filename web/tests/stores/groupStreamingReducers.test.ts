import { describe, expect, it } from "vite-plus/test";

import {
  completeStreamingEventsForActorPatch,
  reconcileStreamingMessagePatch,
  removeStreamingEventPatch,
  upsertStreamingActivityPatch,
  type StreamingChatBucket,
} from "../../src/stores/groupStreamingReducers";
import type { StreamingActivity } from "../../src/types";

function makeBucket(overrides?: Partial<StreamingChatBucket>): StreamingChatBucket {
  return {
    events: [],
    streamingEvents: [],
    streamingTextByStreamId: {},
    streamingActivitiesByStreamId: {},
    replySessionsByPendingEventId: {},
    pendingEventIdByStreamId: {},
    ...overrides,
  };
}

describe("reconcileStreamingMessagePatch", () => {
  it("keeps current activities when message reconciliation arrives without an activity payload", () => {
    const activities: StreamingActivity[] = [
      {
        id: "activity-1",
        kind: "command",
        status: "updated",
        summary: "pwd",
        ts: "2026-04-06T08:00:00.000Z",
      },
    ];
    const bucket = makeBucket({
      streamingEvents: [
        {
          id: "stream:s-1",
          ts: "2026-04-06T08:00:00.000Z",
          kind: "chat.message",
          by: "coder",
          _streaming: true,
          data: {
            text: "",
            to: ["user"],
            stream_id: "s-1",
            pending_event_id: "evt-1",
            pending_placeholder: false,
            activities,
          },
        },
      ],
      streamingActivitiesByStreamId: { "s-1": activities },
      pendingEventIdByStreamId: { "s-1": "evt-1" },
    });

    const patch = reconcileStreamingMessagePatch(bucket, "g-1", "coder", {
      pendingEventId: "evt-1",
      streamId: "s-1",
      ts: "2026-04-06T08:00:01.000Z",
      fullText: "",
      eventText: "",
      activities: [],
      completed: false,
      transientStream: false,
      phase: "commentary",
    });

    expect(patch).not.toBeNull();
    const nextEvent = (patch?.streamingEvents || [])[0];
    expect(
      Array.isArray((nextEvent?.data as { activities?: unknown[] } | undefined)?.activities),
    ).toBe(true);
    expect(
      (nextEvent?.data as { activities?: StreamingActivity[] } | undefined)?.activities || [],
    ).toHaveLength(1);
    expect(
      ((nextEvent?.data as { activities?: StreamingActivity[] } | undefined)?.activities || [])[0]
        ?.summary,
    ).toBe("pwd");
    expect((patch?.streamingActivitiesByStreamId || {})["s-1"]).toHaveLength(1);
  });

  it("keeps the session live when an individual Codex message stream completes before the turn", () => {
    const bucket = makeBucket({
      streamingEvents: [
        {
          id: "stream:stream-commentary",
          ts: "2026-04-09T10:00:00Z",
          kind: "chat.message",
          by: "coder",
          _streaming: true,
          data: {
            text: "Commentary",
            to: ["user"],
            stream_id: "stream-commentary",
            pending_event_id: "evt-codex",
            pending_placeholder: false,
            stream_phase: "commentary",
            activities: [],
          },
        },
      ],
      streamingTextByStreamId: { "stream-commentary": "Commentary" },
      replySessionsByPendingEventId: {
        "evt-codex": {
          pendingEventId: "evt-codex",
          actorId: "coder",
          currentStreamId: "stream-commentary",
          phase: "streaming",
          updatedAt: Date.parse("2026-04-09T10:00:00Z"),
        },
      },
      pendingEventIdByStreamId: { "stream-commentary": "evt-codex" },
    });

    const patch = reconcileStreamingMessagePatch(bucket, "g-1", "coder", {
      pendingEventId: "evt-codex",
      streamId: "stream-commentary",
      ts: "2026-04-09T10:00:01Z",
      fullText: "Commentary",
      eventText: "Commentary",
      activities: [],
      completed: true,
      transientStream: true,
      phase: "commentary",
    });

    expect(patch?.streamingEvents?.[0]?._streaming).toBe(false);
    expect(patch?.replySessionsByPendingEventId?.["evt-codex"]).toMatchObject({
      currentStreamId: "stream-commentary",
      phase: "streaming",
    });
  });

  it("completes sessions on turn completion even when their stream row already completed", () => {
    const bucket = makeBucket({
      streamingEvents: [
        {
          id: "stream:stream-final",
          ts: "2026-04-09T10:00:10Z",
          kind: "chat.message",
          by: "coder",
          _streaming: false,
          data: {
            text: "Final answer",
            to: ["user"],
            stream_id: "stream-final",
            pending_event_id: "evt-codex",
            pending_placeholder: false,
            stream_phase: "final_answer",
            activities: [],
          },
        },
      ],
      streamingTextByStreamId: { "stream-final": "Final answer" },
      replySessionsByPendingEventId: {
        "evt-codex": {
          pendingEventId: "evt-codex",
          actorId: "coder",
          currentStreamId: "stream-final",
          phase: "streaming",
          updatedAt: Date.parse("2026-04-09T10:00:10Z"),
        },
      },
      pendingEventIdByStreamId: { "stream-final": "evt-codex" },
    });

    const patch = completeStreamingEventsForActorPatch(bucket, "coder");

    expect(patch?.streamingEvents?.[0]?._streaming).toBe(false);
    expect(patch?.replySessionsByPendingEventId?.["evt-codex"]).toMatchObject({
      currentStreamId: "stream-final",
      phase: "completed",
    });
  });

  it("does not reopen a completed Codex session when a late stream update arrives", () => {
    const completedAt = Date.parse("2026-04-09T10:00:10Z");
    const bucket = makeBucket({
      streamingEvents: [
        {
          id: "stream:stream-final",
          ts: "2026-04-09T10:00:10Z",
          kind: "chat.message",
          by: "coder",
          _streaming: false,
          data: {
            text: "Final answer",
            to: ["user"],
            stream_id: "stream-final",
            pending_event_id: "evt-codex",
            pending_placeholder: false,
            stream_phase: "final_answer",
            activities: [],
          },
        },
      ],
      streamingTextByStreamId: { "stream-final": "Final answer" },
      replySessionsByPendingEventId: {
        "evt-codex": {
          pendingEventId: "evt-codex",
          actorId: "coder",
          currentStreamId: "stream-final",
          phase: "completed",
          updatedAt: completedAt,
        },
      },
      pendingEventIdByStreamId: { "stream-final": "evt-codex" },
    });

    const patch = reconcileStreamingMessagePatch(bucket, "g-1", "coder", {
      pendingEventId: "evt-codex",
      streamId: "stream-late-commentary",
      ts: "2026-04-09T10:00:20Z",
      fullText: "late commentary replay",
      eventText: "late commentary replay",
      activities: [],
      completed: false,
      transientStream: true,
      phase: "commentary",
    });

    expect(patch?.replySessionsByPendingEventId?.["evt-codex"]).toMatchObject({
      currentStreamId: "stream-final",
      phase: "completed",
      updatedAt: completedAt,
    });
    expect(patch?.streamingEvents).toHaveLength(2);
  });
});

describe("upsertStreamingActivityPatch", () => {
  it("replaces an existing activity with the same id instead of appending a duplicate", () => {
    const bucket = makeBucket({
      streamingEvents: [
        {
          id: "stream:s-1",
          ts: "2026-04-06T08:00:00.000Z",
          kind: "chat.message",
          by: "coder",
          _streaming: true,
          data: {
            text: "",
            to: ["user"],
            stream_id: "s-1",
            pending_event_id: "evt-1",
            pending_placeholder: false,
            activities: [
              {
                id: "reasoning:1",
                kind: "thinking",
                status: "started",
                summary: "initial",
                ts: "2026-04-06T08:00:00.000Z",
              },
            ],
          },
        },
      ],
      streamingActivitiesByStreamId: {
        "s-1": [
          {
            id: "reasoning:1",
            kind: "thinking",
            status: "started",
            summary: "initial",
            ts: "2026-04-06T08:00:00.000Z",
          },
        ],
      },
      replySessionsByPendingEventId: {
        "evt-1": {
          pendingEventId: "evt-1",
          actorId: "coder",
          currentStreamId: "s-1",
          phase: "streaming",
          updatedAt: Date.parse("2026-04-06T08:00:00.000Z"),
        },
      },
      pendingEventIdByStreamId: { "s-1": "evt-1" },
    });

    const patch = upsertStreamingActivityPatch(
      bucket,
      "g-1",
      "coder",
      { streamId: "s-1", pendingEventId: "evt-1" },
      {
        id: "reasoning:1",
        kind: "thinking",
        status: "completed",
        summary: "final",
        ts: "2026-04-06T08:00:01.000Z",
      },
    );

    expect(patch).not.toBeNull();
    const nextActivities = (patch?.streamingActivitiesByStreamId || {})["s-1"] || [];
    expect(nextActivities).toHaveLength(1);
    expect(nextActivities[0]).toMatchObject({
      id: "reasoning:1",
      kind: "thinking",
      status: "completed",
      summary: "final",
      ts: "2026-04-06T08:00:00.000Z",
    });
    const nextEventActivities =
      ((patch?.streamingEvents || [])[0]?.data as { activities?: StreamingActivity[] } | undefined)
        ?.activities || [];
    expect(nextEventActivities).toHaveLength(1);
    expect(nextEventActivities[0]).toMatchObject({
      id: "reasoning:1",
      kind: "thinking",
      status: "completed",
      summary: "final",
      ts: "2026-04-06T08:00:00.000Z",
    });
    expect((patch?.replySessionsByPendingEventId || {})["evt-1"]).toMatchObject({
      currentStreamId: "s-1",
      phase: "streaming",
    });
  });
});

describe("removeStreamingEventPatch", () => {
  it("clears stale stream caches even when the streaming row is already gone", () => {
    const bucket = makeBucket({
      streamingTextByStreamId: { "s-stale": "partial text" },
      streamingActivitiesByStreamId: {
        "s-stale": [
          {
            id: "activity-1",
            kind: "thinking",
            status: "started",
            summary: "investigate",
            ts: "2026-04-06T08:00:00.000Z",
          },
        ],
      },
      replySessionsByPendingEventId: {
        "evt-1": {
          pendingEventId: "evt-1",
          actorId: "coder",
          currentStreamId: "s-stale",
          phase: "streaming",
          updatedAt: Date.parse("2026-04-06T08:00:00.000Z"),
        },
      },
      pendingEventIdByStreamId: { "s-stale": "evt-1" },
    });

    const patch = removeStreamingEventPatch(bucket, "s-stale");

    expect(patch).not.toBeNull();
    expect(patch?.streamingTextByStreamId || {}).not.toHaveProperty("s-stale");
    expect(patch?.streamingActivitiesByStreamId || {}).not.toHaveProperty("s-stale");
    expect(patch?.pendingEventIdByStreamId || {}).not.toHaveProperty("s-stale");
    expect(patch?.replySessionsByPendingEventId || {}).not.toHaveProperty("evt-1");
  });
});
