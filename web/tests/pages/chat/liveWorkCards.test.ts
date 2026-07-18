import { describe, expect, it } from "vite-plus/test";

import { buildLiveWorkCards } from "../../../src/pages/chat/liveWorkCards";
import type { Actor, LedgerEvent } from "../../../src/types";

describe("buildLiveWorkCards", () => {
  it("prefers active headless preview state and ignores PTY actors", () => {
    const actors: Actor[] = [
      { id: "coder", title: "Coder", runtime: "codex", runner: "headless" },
      { id: "shell", title: "Shell", runtime: "codex", runner: "pty" },
    ];
    const events: LedgerEvent[] = [
      {
        id: "stream-1",
        kind: "chat.message",
        ts: "2025-01-01T00:00:00Z",
        by: "coder",
        _streaming: true,
        data: {
          stream_id: "stream-1",
          pending_event_id: "evt-1",
          stream_phase: "commentary",
          pending_placeholder: false,
        },
      },
      {
        id: "stream-pty",
        kind: "chat.message",
        ts: "2025-01-01T00:00:01Z",
        by: "shell",
        _streaming: true,
        data: { stream_id: "stream-pty", pending_event_id: "evt-pty" },
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events,
      latestActorPreviewByActorId: {
        coder: {
          actorId: "coder",
          pendingEventId: "evt-1",
          currentStreamId: "stream-1",
          phase: "streaming",
          streamPhase: "commentary",
          updatedAt: "2025-01-01T00:00:02Z",
          latestText: "Inspecting the reducer chain",
          transcriptBlocks: [
            {
              id: "stream-1::commentary",
              streamId: "stream-1",
              streamPhase: "commentary",
              text: "Inspecting the reducer chain",
              updatedAt: "2025-01-01T00:00:02Z",
              completed: false,
              transient: true,
            },
          ],
          activities: [{ id: "tool-1", kind: "tool", status: "started", summary: "search docs" }],
        },
      },
      latestActorTextByActorId: { coder: "Inspecting the reducer chain" },
      latestActorActivitiesByActorId: {
        coder: [{ id: "tool-1", kind: "tool", status: "started", summary: "search docs" }],
      },
      replySessionsByPendingEventId: {
        "evt-1": {
          pendingEventId: "evt-1",
          actorId: "coder",
          currentStreamId: "stream-1",
          phase: "streaming",
          updatedAt: Date.parse("2025-01-01T00:00:02Z"),
        },
      },
    });

    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({
      actorId: "coder",
      actorLabel: "Coder",
      phase: "streaming",
      streamPhase: "commentary",
      text: "Inspecting the reducer chain",
      streamId: "stream-1",
      pendingEventId: "evt-1",
    });
    expect(cards[0]?.transcriptBlocks).toHaveLength(1);
    expect(cards[0]?.activities.map((item) => item.summary)).toEqual(["search docs"]);
  });

  it("keeps the latest completed headless trace visible when no formal chat message exists", () => {
    const actors: Actor[] = [
      { id: "reviewer", title: "Reviewer", runtime: "claude", runner_effective: "headless" },
      { id: "helper", title: "Helper", runtime: "claude", runner: "headless" },
    ];
    const events: LedgerEvent[] = [
      {
        id: "stream-complete",
        kind: "chat.message",
        ts: "2025-01-02T12:00:00Z",
        by: "reviewer",
        _streaming: false,
        data: {
          stream_id: "stream-complete",
          pending_event_id: "evt-complete",
          stream_phase: "final_answer",
        },
      },
      {
        id: "stream-pending",
        kind: "chat.message",
        ts: "2025-01-02T11:59:00Z",
        by: "helper",
        _streaming: true,
        data: {
          stream_id: "stream-pending",
          pending_event_id: "evt-pending",
          pending_placeholder: true,
        },
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events,
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: { reviewer: "Patch applied and verified" },
      latestActorActivitiesByActorId: {
        helper: [{ id: "queued-1", kind: "queued", status: "started", summary: "queued" }],
      },
      replySessionsByPendingEventId: {
        "evt-complete": {
          pendingEventId: "evt-complete",
          actorId: "reviewer",
          currentStreamId: "stream-complete",
          phase: "completed",
          updatedAt: Date.parse("2025-01-02T12:00:00Z"),
        },
        "evt-pending": {
          pendingEventId: "evt-pending",
          actorId: "helper",
          currentStreamId: "stream-pending",
          phase: "pending",
          updatedAt: Date.parse("2025-01-02T12:01:00Z"),
        },
      },
    });

    expect(cards.map((item) => item.actorId)).toEqual(["helper", "reviewer"]);
    expect(cards[0]).toMatchObject({ actorId: "helper", phase: "pending" });
    expect(cards[1]).toMatchObject({
      actorId: "reviewer",
      phase: "completed",
      streamPhase: "final_answer",
      text: "Patch applied and verified",
    });
  });

  it("ignores ledger-backed chat messages when no live session state exists", () => {
    const actors: Actor[] = [
      { id: "reviewer", title: "Reviewer", runtime: "claude", runner_effective: "headless" },
    ];
    const events: LedgerEvent[] = [
      {
        id: "evt-visible",
        kind: "chat.message",
        ts: "2025-01-02T12:05:00Z",
        by: "reviewer",
        data: { text: "Visible reply", stream_id: "stream-visible" },
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events,
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: {},
      replySessionsByPendingEventId: {},
    });

    expect(cards).toEqual([]);
  });

  it("preserves the recent headless activity trace instead of truncating to three items", () => {
    const actors: Actor[] = [{ id: "coder", title: "Coder", runtime: "codex", runner: "headless" }];
    const events: LedgerEvent[] = [
      {
        id: "stream-activities",
        kind: "chat.message",
        ts: "2025-01-03T00:00:00Z",
        by: "coder",
        _streaming: true,
        data: {
          stream_id: "stream-activities",
          pending_event_id: "evt-activities",
          pending_placeholder: false,
        },
      },
    ];

    const activities = [
      { id: "activity-1", kind: "commentary", status: "started", summary: "Read runtime state" },
      {
        id: "activity-2",
        kind: "commentary",
        status: "started",
        summary: "Compared reducer output",
      },
      { id: "activity-3", kind: "commentary", status: "started", summary: "Matched pending reply" },
      {
        id: "activity-4",
        kind: "commentary",
        status: "started",
        summary: "Prepared preview payload",
      },
      {
        id: "activity-5",
        kind: "commentary",
        status: "started",
        summary: "Waiting for next token",
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events,
      latestActorPreviewByActorId: {},
      latestActorTextByActorId: {},
      latestActorActivitiesByActorId: { coder: activities },
      replySessionsByPendingEventId: {
        "evt-activities": {
          pendingEventId: "evt-activities",
          actorId: "coder",
          currentStreamId: "stream-activities",
          phase: "streaming",
          updatedAt: Date.parse("2025-01-03T00:00:01Z"),
        },
      },
    });

    expect(cards).toHaveLength(1);
    expect(cards[0]?.activities.map((item) => item.id)).toEqual(activities.map((item) => item.id));
  });

  it("uses session-scoped transcript blocks so commentary survives when final answer starts", () => {
    const actors: Actor[] = [{ id: "coder", title: "Coder", runtime: "codex", runner: "headless" }];
    const events: LedgerEvent[] = [
      {
        id: "stream-final",
        kind: "chat.message",
        ts: "2025-01-03T00:00:03Z",
        by: "coder",
        _streaming: true,
        data: {
          stream_id: "stream-final",
          pending_event_id: "evt-transcript",
          stream_phase: "final_answer",
        },
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events,
      latestActorPreviewByActorId: {
        coder: {
          actorId: "coder",
          pendingEventId: "evt-transcript",
          currentStreamId: "stream-final",
          phase: "streaming",
          streamPhase: "final_answer",
          updatedAt: "2025-01-03T00:00:03Z",
          latestText: "Final answer body",
          transcriptBlocks: [
            {
              id: "stream-commentary::commentary",
              streamId: "stream-commentary",
              streamPhase: "commentary",
              text: "Investigating the cache invalidation path",
              updatedAt: "2025-01-03T00:00:01Z",
              completed: true,
              transient: true,
            },
            {
              id: "stream-final::final_answer",
              streamId: "stream-final",
              streamPhase: "final_answer",
              text: "Final answer body",
              updatedAt: "2025-01-03T00:00:03Z",
              completed: false,
              transient: false,
            },
          ],
          activities: [],
        },
      },
      latestActorTextByActorId: { coder: "Final answer body" },
      latestActorActivitiesByActorId: {},
      replySessionsByPendingEventId: {
        "evt-transcript": {
          pendingEventId: "evt-transcript",
          actorId: "coder",
          currentStreamId: "stream-final",
          phase: "streaming",
          updatedAt: Date.parse("2025-01-03T00:00:03Z"),
        },
      },
    });

    expect(cards).toHaveLength(1);
    expect(cards[0]?.transcriptBlocks.map((block) => block.streamPhase)).toEqual([
      "commentary",
      "final_answer",
    ]);
    expect(cards[0]?.text).toBe("Final answer body");
  });

  it("carries recent preview sessions through while using the newest session as the active preview", () => {
    const actors: Actor[] = [{ id: "coder", title: "Coder", runtime: "codex", runner: "headless" }];

    const previewSessions = [
      {
        actorId: "coder",
        pendingEventId: "evt-1",
        currentStreamId: "stream-1",
        phase: "completed",
        streamPhase: "final_answer",
        updatedAt: "2025-01-04T00:00:01Z",
        latestText: "Older answer",
        transcriptBlocks: [
          {
            id: "stream-1::final_answer",
            streamId: "stream-1",
            streamPhase: "final_answer",
            text: "Older answer",
            updatedAt: "2025-01-04T00:00:01Z",
            completed: true,
            transient: false,
          },
        ],
        activities: [],
      },
      {
        actorId: "coder",
        pendingEventId: "evt-2",
        currentStreamId: "stream-2",
        phase: "streaming",
        streamPhase: "commentary",
        updatedAt: "2025-01-04T00:00:03Z",
        latestText: "Current work",
        transcriptBlocks: [
          {
            id: "stream-2::commentary",
            streamId: "stream-2",
            streamPhase: "commentary",
            text: "Current work",
            updatedAt: "2025-01-04T00:00:03Z",
            completed: false,
            transient: true,
          },
        ],
        activities: [
          {
            id: "activity-2",
            kind: "tool",
            status: "started",
            summary: "search docs",
            ts: "2025-01-04T00:00:02Z",
          },
        ],
      },
    ];

    const cards = buildLiveWorkCards({
      actors,
      events: [
        {
          id: "stream-2",
          kind: "chat.message",
          ts: "2025-01-04T00:00:03Z",
          by: "coder",
          _streaming: true,
          data: { stream_id: "stream-2", pending_event_id: "evt-2", stream_phase: "commentary" },
        },
      ],
      latestActorPreviewByActorId: { coder: previewSessions[1] },
      previewSessionsByActorId: { coder: previewSessions },
      latestActorTextByActorId: { coder: "Current work" },
      latestActorActivitiesByActorId: {
        coder: [
          {
            id: "activity-2",
            kind: "tool",
            status: "started",
            summary: "search docs",
            ts: "2025-01-04T00:00:02Z",
          },
        ],
      },
      replySessionsByPendingEventId: {
        "evt-2": {
          pendingEventId: "evt-2",
          actorId: "coder",
          currentStreamId: "stream-2",
          phase: "streaming",
          updatedAt: Date.parse("2025-01-04T00:00:03Z"),
        },
      },
    });

    expect(cards).toHaveLength(1);
    expect(cards[0]?.text).toBe("Current work");
    expect(cards[0]?.pendingEventId).toBe("evt-2");
    expect(cards[0]?.previewSessions?.map((session) => session.pendingEventId)).toEqual([
      "evt-1",
      "evt-2",
    ]);
  });
});
