import { describe, expect, it } from "vitest";

import {
  createRuntimeDockTickerCache,
  hasRuntimeDockTickerWork,
  pruneRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
} from "../../../src/pages/chat/runtimeDockTickerCache";
import { buildRuntimeDockTickerEntries } from "../../../src/pages/chat/runtimeDockTickerEntries";
import type { RuntimeDockTickerEntry } from "../../../src/pages/chat/runtimeDockTickerEntries";
import type { LiveWorkCard } from "../../../src/pages/chat/liveWorkCards";
import { buildRuntimeDockItems, type RuntimeDockItem } from "../../../src/pages/chat/runtimeDockItems";
import { getRuntimeRingTone } from "../../../src/pages/chat/runtimeDockRingTone";
import type { Actor, HeadlessPreviewBlock, StreamingActivity } from "../../../src/types";

function makeActivity(args: {
  id: string;
  kind?: StreamingActivity["kind"];
  status?: StreamingActivity["status"];
  summary: string;
  ts?: string;
}): StreamingActivity {
  return {
    kind: "thinking",
    status: "updated",
    ...args,
  };
}

function makeMessageBlock(args: {
  id: string;
  text: string;
  streamPhase?: string;
  updatedAt?: string;
  completed?: boolean;
  transient?: boolean;
}): HeadlessPreviewBlock {
  return {
    id: args.id,
    streamId: `stream-${args.id}`,
    streamPhase: args.streamPhase || "commentary",
    text: args.text,
    updatedAt: args.updatedAt || "2025-01-02T12:00:00Z",
    completed: Boolean(args.completed),
    transient: args.transient ?? true,
  };
}

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
      ? [{
          actorId: args.actorId,
          pendingEventId: `pending-${args.actorId}`,
          currentStreamId: `stream-${args.actorId}`,
          phase: args.phase || "streaming",
          streamPhase: args.streamPhase || "commentary",
          updatedAt: args.updatedAt || "2025-01-02T12:00:00Z",
          latestText: String(args.previewBlocks?.[args.previewBlocks.length - 1]?.text || ""),
          transcriptBlocks: args.previewBlocks || [],
          activities: args.previewActivities || [],
        }]
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
      { id: "coder", title: "Coder", runtime: "codex", runner: "pty", runner_effective: "headless" },
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

  it("builds newest ticker entries from active headless preview sessions", () => {
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "coder",
        actorLabel: "Coder",
        liveWorkCard: makeLiveWorkCard({
          actorId: "coder",
          actorLabel: "Coder",
          activities: [
            makeActivity({
              id: "queued",
              kind: "queued",
              status: "started",
              summary: "Queued",
              ts: "2025-01-02T12:00:00Z",
            }),
            makeActivity({
              id: "read-source",
              kind: "tool",
              summary: "Read source",
              ts: "2025-01-02T12:03:56Z",
            }),
            makeActivity({
              id: "typecheck",
              kind: "command",
              summary: "Ran typecheck",
              ts: "2025-01-02T12:03:58Z",
            }),
          ],
          previewBlocks: [
            makeMessageBlock({
              id: "commentary",
              text: "Reading the current dock implementation.",
              updatedAt: "2025-01-02T12:03:57Z",
            }),
            makeMessageBlock({
              id: "final-answer",
              text: "I found the ticker data source issue.",
              streamPhase: "final_answer",
              updatedAt: "2025-01-02T12:03:59Z",
            }),
          ],
          previewActivities: [
            makeActivity({
              id: "inspect",
              kind: "tool",
              summary: "Inspected RuntimeDock",
              ts: "2025-01-02T12:03:59Z",
            }),
          ],
        }),
      }),
      makeRuntimeDockItem({
        actorId: "reviewer",
        actorLabel: "Reviewer",
        liveWorkCard: makeLiveWorkCard({
          actorId: "reviewer",
          actorLabel: "Reviewer",
          phase: "pending",
          activities: [
            makeActivity({
              id: "prepare",
              kind: "plan",
              summary: "Preparing response",
              ts: "2025-01-02T12:04:00Z",
            }),
          ],
          previewActivities: [
            makeActivity({
              id: "prepare",
              kind: "plan",
              summary: "Preparing response",
              ts: "2025-01-02T12:04:00Z",
            }),
          ],
        }),
      }),
    ];

    expect(buildRuntimeDockTickerEntries(items, 4).map((entry) => [entry.kind, entry.actorLabel, entry.text])).toEqual([
      ["activity", "Reviewer", "Preparing response"],
      ["message", "Coder", "I found the ticker data source issue."],
      ["activity", "Coder", "Inspected RuntimeDock"],
    ]);
    expect(buildRuntimeDockTickerEntries(items, 2).map((entry) => [entry.actorLabel, entry.text])).toEqual([
      ["Reviewer", "Preparing response"],
      ["Coder", "I found the ticker data source issue."],
    ]);
  });

  it("ignores inactive live work cards and dedupes repeated activity entries", () => {
    const repeatedActivity = makeActivity({
      id: "same",
      kind: "tool",
      summary: "Reading logs",
      ts: "2025-01-02T12:00:00Z",
    });
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "done",
        actorLabel: "Done",
        liveWorkCard: makeLiveWorkCard({
          actorId: "done",
          actorLabel: "Done",
          phase: "completed",
          activities: [
            makeActivity({
              id: "completed",
              kind: "reply",
              summary: "Completed work",
              ts: "2025-01-02T12:10:00Z",
            }),
          ],
        }),
      }),
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          activities: [repeatedActivity],
          previewActivities: [repeatedActivity],
        }),
      }),
    ];

    expect(buildRuntimeDockTickerEntries(items)).toMatchObject([
      {
        kind: "activity",
        actorId: "active",
        actorLabel: "Active",
        text: "Reading logs",
      },
    ]);
  });

  it("uses live work card fields when preview sessions are not available", () => {
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          text: "Top-level transcript",
          activities: [
            makeActivity({
              id: "top-level",
              kind: "tool",
              summary: "Top-level fallback",
              ts: "2025-01-02T12:00:00Z",
            }),
          ],
        }),
      }),
    ];

    expect(buildRuntimeDockTickerEntries(items).map((entry) => [entry.kind, entry.text])).toEqual([
      ["message", "Top-level transcript"],
      ["activity", "Top-level fallback"],
    ]);
  });

  it("keeps old preview entries in helper output so the UI cache owns expiry timing", () => {
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          previewActivities: [
            makeActivity({
              id: "old",
              kind: "tool",
              summary: "Old faded step",
              ts: "2025-01-02T12:00:00Z",
            }),
            makeActivity({
              id: "new",
              kind: "tool",
              summary: "Fresh step",
              ts: "2025-01-02T12:00:08Z",
            }),
          ],
        }),
      }),
    ];

    expect(buildRuntimeDockTickerEntries(items).map((entry) => entry.text)).toEqual(["Fresh step", "Old faded step"]);
  });

  it("preserves long streaming message text without summarizing it", () => {
    const longText = "A".repeat(150);
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          previewBlocks: [
            makeMessageBlock({
              id: "long",
              text: longText,
              updatedAt: "2025-01-02T12:00:00Z",
            }),
          ],
        }),
      }),
    ];

    const entries = buildRuntimeDockTickerEntries(items);
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      kind: "message",
      text: longText,
      sourceId: "message:active:pending-active",
    });
  });

  it("keeps only the newest transcript block for one preview session", () => {
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          previewBlocks: [
            makeMessageBlock({
              id: "fragment-1",
              text: "项目总监：，",
              updatedAt: "2025-01-02T12:00:00Z",
            }),
            makeMessageBlock({
              id: "fragment-2",
              text: "项目总监：不能把整段历史消息都做字符级动画。",
              updatedAt: "2025-01-02T12:00:01Z",
            }),
          ],
        }),
      }),
    ];

    expect(buildRuntimeDockTickerEntries(items)).toMatchObject([
      {
        kind: "message",
        actorId: "active",
        text: "项目总监：不能把整段历史消息都做字符级动画。",
        sourceId: "message:active:pending-active",
      },
    ]);
  });

  it("splits long streaming message deltas without summarizing them", () => {
    const cache = createRuntimeDockTickerCache();
    const longText = "A".repeat(220);
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: longText,
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
    };

    const firstVisible = upsertRuntimeDockTickerCache(cache, [entry], 1000);
    const secondVisible = pruneRuntimeDockTickerCache(cache, 1250);
    const thirdVisible = pruneRuntimeDockTickerCache(cache, 1500);

    expect(firstVisible).toHaveLength(1);
    expect(firstVisible[0]?.text.length).toBeGreaterThan(56);
    expect(secondVisible).toHaveLength(2);
    expect(thirdVisible).toHaveLength(3);
    expect(thirdVisible.map((visibleEntry) => visibleEntry.text).join("")).toBe(longText);
  });

  it("preserves explicit message lines as separate cache entries", () => {
    const items: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          previewBlocks: [
            makeMessageBlock({
              id: "multi-line",
              text: "First line\nSecond line",
              updatedAt: "2025-01-02T12:00:00Z",
            }),
          ],
        }),
      }),
    ];

    const cache = createRuntimeDockTickerCache();
    const firstVisible = upsertRuntimeDockTickerCache(cache, buildRuntimeDockTickerEntries(items), 1000);
    const secondVisible = pruneRuntimeDockTickerCache(cache, 1250);
    expect(firstVisible.map((entry) => entry.text)).toEqual([
      "First line",
    ]);
    expect(secondVisible.map((entry) => entry.text)).toEqual([
      "First line",
      "Second line",
    ]);
  });

  it("buffers a short growing message until it reaches a natural boundary", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "hello",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
    };

    const firstVisible = upsertRuntimeDockTickerCache(cache, [entry], 1000);
    const secondVisible = upsertRuntimeDockTickerCache(cache, [{
      ...entry,
      text: "hello world.",
      updatedAt: "2025-01-02T12:00:01Z",
    }], 1200);

    expect(firstVisible).toEqual([]);
    expect(secondVisible.map((visibleEntry) => visibleEntry.text)).toEqual(["hello world."]);
    expect(new Set(secondVisible.map((visibleEntry) => visibleEntry.id)).size).toBe(1);
  });

  it("does not flush a tiny incomplete message only because the ticker delay elapsed", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "thinking",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
    };

    expect(upsertRuntimeDockTickerCache(cache, [entry], 1000)).toEqual([]);
    expect(pruneRuntimeDockTickerCache(cache, 1421)).toEqual([]);
    expect(hasRuntimeDockTickerWork(cache)).toBe(false);
  });

  it("keeps ticker work pending only while buffered text can advance or visible entries can expire", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "investigating ticker rendering",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
      completed: false,
    };

    expect(upsertRuntimeDockTickerCache(cache, [entry], 1000)).toEqual([]);
    expect(hasRuntimeDockTickerWork(cache)).toBe(true);
    expect(pruneRuntimeDockTickerCache(cache, 1420).map((visibleEntry) => visibleEntry.text)).toEqual([
      "investigating ticker rendering",
    ]);
    expect(hasRuntimeDockTickerWork(cache)).toBe(true);
    expect(pruneRuntimeDockTickerCache(cache, 6421)).toEqual([]);
    expect(hasRuntimeDockTickerWork(cache)).toBe(false);
  });

  it("flushes a short message when the stream marks it completed", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "thinking",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
      completed: false,
    };

    expect(upsertRuntimeDockTickerCache(cache, [entry], 1000)).toEqual([]);
    expect(upsertRuntimeDockTickerCache(cache, [{ ...entry, completed: true }], 1200).map((visibleEntry) => visibleEntry.text)).toEqual([
      "thinking",
    ]);
  });

  it("lets completed transcript cards flush a buffered final ticker fragment", () => {
    const cache = createRuntimeDockTickerCache();
    const streamingItems: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "streaming",
          previewBlocks: [
            makeMessageBlock({
              id: "tail",
              text: "thinking",
              completed: false,
            }),
          ],
        }),
      }),
    ];
    const completedItems: RuntimeDockItem[] = [
      makeRuntimeDockItem({
        actorId: "active",
        actorLabel: "Active",
        liveWorkCard: makeLiveWorkCard({
          actorId: "active",
          actorLabel: "Active",
          phase: "completed",
          previewBlocks: [
            makeMessageBlock({
              id: "tail",
              text: "thinking",
              completed: true,
              transient: false,
            }),
          ],
        }),
      }),
    ];

    expect(upsertRuntimeDockTickerCache(cache, buildRuntimeDockTickerEntries(streamingItems), 1000)).toEqual([]);
    const completedEntries = buildRuntimeDockTickerEntries(completedItems);
    expect(completedEntries).toMatchObject([
      {
        kind: "message",
        text: "thinking",
        completed: true,
      },
    ]);
    expect(upsertRuntimeDockTickerCache(cache, completedEntries, 1200).map((entry) => entry.text)).toEqual([
      "thinking",
    ]);
  });

  it("flushes slow-growing transcript text in readable chunks instead of tiny fragments", () => {
    const cache = createRuntimeDockTickerCache();
    const longText = "我已经在真实 UI 里抓到现象了，现在正在继续检查 ticker 的 transcript 展示是否会被过早截断。";
    let visible: RuntimeDockTickerEntry[] = [];

    for (let index = 1; index <= longText.length; index += 1) {
      const nowMs = 1000 + index * 100;
      visible = upsertRuntimeDockTickerCache(cache, [{
        id: "message:active:pending:block",
        kind: "message",
        actorId: "active",
        actorLabel: "Active",
        text: longText.slice(0, index),
        updatedAt: "2025-01-02T12:00:00Z",
        sourceId: "message:active:pending:block",
        completed: false,
      }], nowMs);
    }

    const visibleText = visible.map((visibleEntry) => visibleEntry.text).join("");
    expect(visible.length).toBeGreaterThan(1);
    expect(visibleText).toBe(longText);
    expect(visible.every((visibleEntry) => visibleEntry.text.length >= 12 || visibleEntry.text.endsWith("。"))).toBe(true);
  });

  it("flushes punctuation-free transcript text after the timed delay", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "investigating ticker rendering",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
      completed: false,
    };

    expect(upsertRuntimeDockTickerCache(cache, [entry], 1000)).toEqual([]);
    expect(pruneRuntimeDockTickerCache(cache, 1300)).toEqual([]);
    expect(pruneRuntimeDockTickerCache(cache, 1420).map((visibleEntry) => visibleEntry.text)).toEqual([
      "investigating ticker rendering",
    ]);
  });

  it("does not reinsert an expired ticker entry when old preview history is returned again", () => {
    const cache = createRuntimeDockTickerCache();
    const expiredEntry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block-old",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "Old line.",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block-old",
    };
    const freshEntry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block-fresh",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "Fresh line.",
      updatedAt: "2025-01-02T12:00:06Z",
      sourceId: "message:active:pending:block-fresh",
    };

    expect(upsertRuntimeDockTickerCache(cache, [expiredEntry], 1000).map((entry) => entry.text)).toEqual(["Old line."]);
    expect(pruneRuntimeDockTickerCache(cache, 7001)).toEqual([]);
    expect(upsertRuntimeDockTickerCache(cache, [freshEntry, expiredEntry], 7002).map((entry) => entry.text)).toEqual(["Fresh line."]);
  });

  it("allows an expired ticker entry id to reappear when its streamed text changes", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "Draft.",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
    };
    const updatedEntry = {
      ...entry,
      text: "Draft updated.",
      updatedAt: "2025-01-02T12:00:08Z",
    };

    upsertRuntimeDockTickerCache(cache, [entry], 1000);
    pruneRuntimeDockTickerCache(cache, 7001);

    expect(upsertRuntimeDockTickerCache(cache, [updatedEntry], 7002).map((visibleEntry) => visibleEntry.text)).toEqual(["Draft updated."]);
  });

  it("starts a new message revision when the accumulated text is rewritten", () => {
    const cache = createRuntimeDockTickerCache();
    const entry: RuntimeDockTickerEntry = {
      id: "message:active:pending:block",
      kind: "message",
      actorId: "active",
      actorLabel: "Active",
      text: "First draft.",
      updatedAt: "2025-01-02T12:00:00Z",
      sourceId: "message:active:pending:block",
    };

    upsertRuntimeDockTickerCache(cache, [entry], 1000);
    const visible = upsertRuntimeDockTickerCache(cache, [{
      ...entry,
      text: "Rewritten answer.",
      updatedAt: "2025-01-02T12:00:01Z",
    }], 1200);

    expect(visible.map((visibleEntry) => visibleEntry.text)).toEqual([
      "First draft.",
      "Rewritten answer.",
    ]);
  });

  it("maps runtime dock ring tone from the collapsed active/stopped/attention contract", () => {
    const failedItem = makeRuntimeDockItem({
      actorId: "failed",
      actorLabel: "Failed",
      liveWorkCard: makeLiveWorkCard({
        actorId: "failed",
        actorLabel: "Failed",
        phase: "failed",
      }),
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
    const appServerPtyItem = makeRuntimeDockItem({
      actorId: "pty-app",
      actorLabel: "PTY App",
      liveWorkCard: null,
      runner: "pty",
      runtimeStateSource: "app_server",
    });

    expect(getRuntimeRingTone(failedItem, false, "idle")).toBe("attention");
    expect(getRuntimeRingTone(pendingItem, false, "idle")).toBe("stopped");
    expect(getRuntimeRingTone(pendingItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(streamingFinalAnswerItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(completedFinalAnswerItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(headlessItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(headlessItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(headlessItem, true, "stuck")).toBe("attention");
    expect(getRuntimeRingTone(appServerPtyItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(appServerPtyItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(appServerPtyItem, true, "stuck")).toBe("attention");
    expect(getRuntimeRingTone(ptyItem, true, "working")).toBe("active");
    expect(getRuntimeRingTone(ptyItem, true, "waiting")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, true, "stuck")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, true, "idle")).toBe("idle");
    expect(getRuntimeRingTone(ptyItem, false, "working")).toBe("stopped");
  });
});
