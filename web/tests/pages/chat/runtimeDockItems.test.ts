import { describe, expect, it } from "vite-plus/test";

import { areRuntimeDockTickerEntriesEqual } from "../../../src/pages/chat/runtimeDockTickerCache";
import type { RuntimeDockTickerEntry } from "../../../src/pages/chat/runtimeDockTickerEntries";
import type { LiveWorkCard } from "../../../src/pages/chat/liveWorkCards";
import {
  buildRuntimeDockItems,
  type RuntimeDockItem,
} from "../../../src/pages/chat/runtimeDockItems";
import { getRuntimeRingTone } from "../../../src/pages/chat/runtimeDockRingTone";
import type { Actor, HeadlessPreviewBlock, StreamingActivity } from "../../../src/types";

describe("runtime dock ticker projections", () => {
  it("recognizes unchanged projections so polling can preserve the rendered glass layer", () => {
    const entry: RuntimeDockTickerEntry = {
      id: "activity:actor-1",
      kind: "activity",
      actorId: "actor-1",
      actorLabel: "Actor 1",
      text: "Working",
      receivedAt: 1000,
      updatedAt: "2025-01-02T12:00:00Z",
    };

    expect(areRuntimeDockTickerEntriesEqual([entry], [{ ...entry }])).toBe(true);
    expect(areRuntimeDockTickerEntriesEqual([entry], [{ ...entry, text: "Waiting" }])).toBe(false);
  });
});

function makeLiveWorkCard(args: {
  actorId: string;
  actorLabel: string;
  phase?: LiveWorkCard["phase"];
  streamPhase?: string;
  activities?: StreamingActivity[];
  text?: string;
  transcriptBlocks?: HeadlessPreviewBlock[];
  previewBlocks?: HeadlessPreviewBlock[];
  previewActivities?: StreamingActivity[];
  updatedAt?: string;
}): LiveWorkCard {
  const hasPreviewSession = Boolean(args.previewBlocks || args.previewActivities);
  return {
    actorId: args.actorId,
    actorLabel: args.actorLabel,
    runtime: "codex",
    phase: args.phase || "streaming",
    streamPhase: args.streamPhase || "commentary",
    text: args.text || "",
    transcriptBlocks: args.transcriptBlocks || [],
    activities: args.activities || [],
    previewSessions: hasPreviewSession
      ? [
          {
            actorId: args.actorId,
            pendingEventId: `pending-${args.actorId}`,
            currentStreamId: `stream-${args.actorId}`,
            phase: args.phase || "streaming",
            streamPhase: args.streamPhase || "commentary",
            updatedAt: args.updatedAt || "2025-01-02T12:00:00Z",
            latestText: String(args.previewBlocks?.[args.previewBlocks.length - 1]?.text || ""),
            transcriptBlocks: args.previewBlocks || [],
            activities: args.previewActivities || [],
          },
        ]
      : [],
    updatedAt: args.updatedAt || "2025-01-02T12:00:00Z",
    streamId: `stream-${args.actorId}`,
    pendingEventId: `pending-${args.actorId}`,
  };
}

function makeRuntimeDockItem(args: {
  actorId: string;
  actorLabel: string;
  liveWorkCard: LiveWorkCard | null;
  runner?: RuntimeDockItem["runner"];
  runtimeStateSource?: Actor["runtime_state_source"];
}): RuntimeDockItem {
  const runner = args.runner || "headless";
  const actor: Actor = {
    id: args.actorId,
    title: args.actorLabel,
    runtime: "codex",
    runner,
    runtime_state_source: args.runtimeStateSource,
  };
  return {
    actor,
    actorId: args.actorId,
    actorLabel: args.actorLabel,
    runtime: "codex",
    runner,
    unreadCount: 0,
    webModelQueuedCount: 0,
    liveWorkCard: args.liveWorkCard,
  };
}

describe("runtimeDockItems", () => {
  it("preserves runtime actor order while attaching live work to headless actors", () => {
    const actors: Actor[] = [
      { id: "shell", title: "Shell", runtime: "codex", runner: "pty", unread_count: 2 },
      { id: "coder", title: "Coder", runtime: "codex", runner: "headless" },
      { id: "reviewer", title: "Reviewer", runtime: "claude", runner_effective: "headless" },
    ];

    const items = buildRuntimeDockItems({
      actors,
      liveWorkCards: [
        {
          actorId: "reviewer",
          actorLabel: "Reviewer",
          runtime: "claude",
          phase: "completed",
          streamPhase: "final_answer",
          text: "Review complete",
          transcriptBlocks: [],
          activities: [],
          updatedAt: "2025-01-02T12:00:00Z",
          streamId: "stream-reviewer",
          pendingEventId: "evt-reviewer",
        },
      ],
    });

    expect(items.map((item) => item.actorId)).toEqual(["shell", "coder", "reviewer"]);
    expect(items[0]).toMatchObject({ runner: "pty", unreadCount: 2, liveWorkCard: null });
    expect(items[1]).toMatchObject({ runner: "headless", liveWorkCard: null });
    expect(items[2]?.liveWorkCard?.text).toBe("Review complete");
  });

  it("uses runner_effective when deciding whether an actor is headless", () => {
    const actors: Actor[] = [
      { id: "shell", title: "Shell", runtime: "codex", runner: "pty" },
      {
        id: "coder",
        title: "Coder",
        runtime: "codex",
        runner: "pty",
        runner_effective: "headless",
      },
    ];

    const items = buildRuntimeDockItems({
      actors,
      liveWorkCards: [
        {
          actorId: "coder",
          actorLabel: "Coder",
          runtime: "codex",
          phase: "streaming",
          streamPhase: "commentary",
          text: "Investigating",
          transcriptBlocks: [],
          activities: [],
          updatedAt: "2025-01-02T12:00:01Z",
          streamId: "stream-coder",
          pendingEventId: "evt-coder",
        },
      ],
    });

    expect(items).toHaveLength(2);
    expect(items[1]).toMatchObject({ runner: "headless" });
    expect(items[1]?.liveWorkCard?.streamPhase).toBe("commentary");
  });

  it("maps runtime dock ring tone from the collapsed active/stopped/attention contract", () => {
    const failedItem = makeRuntimeDockItem({
      actorId: "failed",
      actorLabel: "Failed",
      liveWorkCard: makeLiveWorkCard({ actorId: "failed", actorLabel: "Failed", phase: "failed" }),
    });
    const pendingItem = makeRuntimeDockItem({
      actorId: "pending",
      actorLabel: "Pending",
      liveWorkCard: makeLiveWorkCard({
        actorId: "pending",
        actorLabel: "Pending",
        phase: "pending",
      }),
    });
    const streamingFinalAnswerItem = makeRuntimeDockItem({
      actorId: "streaming",
      actorLabel: "Streaming",
      liveWorkCard: makeLiveWorkCard({
        actorId: "streaming",
        actorLabel: "Streaming",
        phase: "streaming",
        streamPhase: "final_answer",
      }),
    });
    const completedFinalAnswerItem = makeRuntimeDockItem({
      actorId: "completed",
      actorLabel: "Completed",
      liveWorkCard: makeLiveWorkCard({
        actorId: "completed",
        actorLabel: "Completed",
        phase: "completed",
        streamPhase: "final_answer",
      }),
    });
    const headlessItem = makeRuntimeDockItem({
      actorId: "headless",
      actorLabel: "Headless",
      liveWorkCard: null,
      runner: "headless",
    });
    const ptyItem = makeRuntimeDockItem({
      actorId: "pty",
      actorLabel: "PTY",
      liveWorkCard: null,
      runner: "pty",
    });
    const managedPtyItem = makeRuntimeDockItem({
      actorId: "pty-managed",
      actorLabel: "PTY Managed",
      liveWorkCard: null,
      runner: "pty",
      runtimeStateSource: "managed_session",
    });

    expect(getRuntimeRingTone(failedItem, false, "idle")).toBe("attention");
    expect(getRuntimeRingTone(pendingItem, false, "idle")).toBe("stopped");
    expect(getRuntimeRingTone(pendingItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(streamingFinalAnswerItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(completedFinalAnswerItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(headlessItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(headlessItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(headlessItem, true, "stuck")).toBe("attention");
    expect(getRuntimeRingTone(managedPtyItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(managedPtyItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(managedPtyItem, true, "stuck")).toBe("attention");
    expect(getRuntimeRingTone(ptyItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(ptyItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, true, "stuck")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, false, "working")).toBe("stopped");
  });
});
