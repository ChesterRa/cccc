import { describe, expect, it } from "vite-plus/test";

import {
  dedupeStreamingActivities,
  mergeStreamingActivity,
  upsertReplySession,
} from "../../src/stores/chatStreamingSessions";
import type { StreamingActivity } from "../../src/types";

describe("chatStreamingSessions", () => {
  it("merges richer activity metadata instead of overwriting it", () => {
    const started: StreamingActivity = {
      id: "command:cmd-1",
      kind: "command",
      status: "started",
      summary: "npm run typecheck",
      command: "npm run typecheck",
      cwd: "/repo/web",
      raw_item_type: "commandExecution",
      ts: "2026-04-09T10:00:00Z",
    };
    const updated: StreamingActivity = {
      id: "command:cmd-1",
      kind: "command",
      status: "updated",
      summary: "typecheck started",
      detail: "Found 0 errors so far",
      ts: "2026-04-09T10:00:05Z",
    };

    expect(mergeStreamingActivity(started, updated)).toEqual({
      id: "command:cmd-1",
      kind: "command",
      status: "updated",
      summary: "typecheck started",
      detail: "Found 0 errors so far",
      ts: "2026-04-09T10:00:00Z",
      raw_item_type: "commandExecution",
      command: "npm run typecheck",
      cwd: "/repo/web",
    });
  });

  it("dedupes activity ids while keeping the earliest order and latest details", () => {
    const activities: StreamingActivity[] = [
      {
        id: "tool:1",
        kind: "tool",
        status: "started",
        summary: "filesystem:read_file",
        tool_name: "read_file",
        server_name: "filesystem",
        ts: "2026-04-09T10:00:00Z",
      },
      {
        id: "tool:2",
        kind: "search",
        status: "started",
        summary: "src/**/*.ts",
        query: "src/**/*.ts",
        ts: "2026-04-09T10:00:01Z",
      },
      {
        id: "tool:1",
        kind: "tool",
        status: "completed",
        summary: "Opened reducer file",
        detail: "web/src/stores/groupStreamingReducers.ts",
        ts: "2026-04-09T10:00:02Z",
      },
    ];

    expect(dedupeStreamingActivities(activities)).toEqual([
      {
        id: "tool:1",
        kind: "tool",
        status: "completed",
        summary: "Opened reducer file",
        detail: "web/src/stores/groupStreamingReducers.ts",
        ts: "2026-04-09T10:00:00Z",
        tool_name: "read_file",
        server_name: "filesystem",
      },
      {
        id: "tool:2",
        kind: "search",
        status: "started",
        summary: "src/**/*.ts",
        ts: "2026-04-09T10:00:01Z",
        query: "src/**/*.ts",
      },
    ]);
  });

  it("keeps terminal reply sessions closed when late stream updates replay", () => {
    const completedAt = Date.parse("2026-04-09T10:00:10Z");
    const result = upsertReplySession(
      {
        "evt-codex": {
          pendingEventId: "evt-codex",
          actorId: "coder",
          currentStreamId: "stream-final",
          phase: "completed",
          updatedAt: completedAt,
        },
      },
      { "stream-final": "evt-codex" },
      {
        pendingEventId: "evt-codex",
        actorId: "coder",
        streamId: "stream-late-delta",
        phase: "streaming",
        updatedAt: Date.parse("2026-04-09T10:00:20Z"),
      },
    );

    expect(result.replySessionsByPendingEventId["evt-codex"]).toMatchObject({
      currentStreamId: "stream-final",
      phase: "completed",
      updatedAt: completedAt,
    });
    expect(result.pendingEventIdByStreamId["stream-late-delta"]).toBe("evt-codex");
  });
});
