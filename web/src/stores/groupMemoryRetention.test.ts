import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import {
  buildChatBucketPatch,
  EMPTY_CHAT_BUCKET,
  ensureGroupChatBucket,
  getCachedGroupView,
  groupViewCache,
  saveGroupView,
  type GroupChatBucket,
  type GroupStateSnapshot,
} from "./groupStoreCore";

import {
  completeStreamingEventsForActorPatch,
  upsertStreamingEventPatch,
} from "./groupStreamingReducers";

function state(chatByGroup: Record<string, GroupChatBucket>): GroupStateSnapshot {
  return {
    groups: [],
    selectedGroupId: "active",
    chatByGroup,
    groupDoc: null,
    actors: [],
    groupContext: null,
    groupSettings: null,
    groupPresentation: null,
  };
}

describe("group memory retention", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    groupViewCache.clear();
  });
  afterEach(() => {
    groupViewCache.clear();
    vi.useRealTimers();
  });

  it("releases expired snapshots without waiting for another group read", () => {
    saveGroupView("old", {
      events: [{ id: "large", kind: "chat.message", data: { text: "payload" } }],
    });
    vi.advanceTimersByTime(300_001);
    expect(groupViewCache.size).toBe(0);
  });

  it("bounds snapshots and keeps recently read groups", () => {
    for (let i = 0; i < 8; i++) saveGroupView(`g${i}`, {});
    expect(getCachedGroupView("g0")).not.toBeNull();
    saveGroupView("new", {});
    expect(groupViewCache.size).toBe(8);
    expect(groupViewCache.has("g0")).toBe(true);
    expect(groupViewCache.has("g1")).toBe(false);
  });

  it("does not extend a snapshot lifetime merely by reading it", () => {
    saveGroupView("old", {});
    vi.advanceTimersByTime(299_000);
    expect(getCachedGroupView("old")).not.toBeNull();
    vi.advanceTimersByTime(1001);
    expect(groupViewCache.has("old")).toBe(false);
  });

  it("clears its expiration timer when the snapshots are cleared", () => {
    const timers = vi.getTimerCount();
    saveGroupView("old", {});
    expect(vi.getTimerCount()).toBe(timers + 1);
    groupViewCache.clear();
    expect(vi.getTimerCount()).toBe(timers);
  });

  it("keeps only recent chat groups while preserving cached events on revisit", () => {
    let buckets: Record<string, GroupChatBucket> = {};
    for (let i = 0; i < 8; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
    buckets.g0 = { ...EMPTY_CHAT_BUCKET, events: [{ id: "retained", kind: "chat.message" }] };
    buckets = ensureGroupChatBucket(buckets, "g0");
    buckets = ensureGroupChatBucket(buckets, "new");
    expect(Object.keys(buckets)).toHaveLength(8);
    expect(buckets.g0.events[0].id).toBe("retained");
    expect(buckets.g1).toBeUndefined();
  });

  it("protects the selected group from late updates for another group", () => {
    let buckets: Record<string, GroupChatBucket> = { active: { ...EMPTY_CHAT_BUCKET } };
    for (let i = 0; i < 7; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
    const patch = buildChatBucketPatch(state(buckets), "late", { events: [] });
    expect(Object.keys(patch?.chatByGroup || {})).toHaveLength(8);
    expect(patch?.chatByGroup?.active).toBe(buckets.active);
    expect(patch?.chatByGroup?.late).toBeDefined();
  });

  it("does not evict streaming or in-flight history reads", () => {
    let buckets: Record<string, GroupChatBucket> = {
      streaming: {
        ...EMPTY_CHAT_BUCKET,
        streamingEvents: [{ id: "live", kind: "chat.message", _streaming: true }],
      },
      loading: { ...EMPTY_CHAT_BUCKET, isLoadingHistory: true },
    };
    for (let i = 0; i < 20; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
    expect(Object.keys(buckets)).toHaveLength(8);
    expect(buckets.streaming.streamingEvents[0].id).toBe("live");
    expect(buckets.loading.isLoadingHistory).toBe(true);
  });

  it("evicts completed output after real streaming lifecycle transitions", () => {
    let buckets: Record<string, GroupChatBucket> = {};
    for (let i = 0; i < 20; i++) {
      const gid = `g${i}`;
      buckets = ensureGroupChatBucket(buckets, gid);
      const started = upsertStreamingEventPatch(buckets[gid], gid, {
        id: `event${i}`,
        kind: "chat.message",
        by: "actor",
        _streaming: true,
        data: { stream_id: `stream${i}`, pending_event_id: `pending${i}`, text: "output" },
      });
      buckets = buildChatBucketPatch(state(buckets), gid, started!)!.chatByGroup!;
      const completed = completeStreamingEventsForActorPatch(buckets[gid], "actor");
      buckets = buildChatBucketPatch(state(buckets), gid, completed!)!.chatByGroup!;
    }
    expect(Object.keys(buckets)).toHaveLength(8);
    expect(buckets.g0).toBeUndefined();
    expect(buckets.g19.streamingEvents[0]._streaming).toBe(false);
    expect(buckets.g19.replySessionsByPendingEventId.pending19.phase).toBe("completed");
  });

  it.each(["pending", "streaming"] as const)(
    "protects a %s reply session between output events",
    (phase) => {
      let buckets: Record<string, GroupChatBucket> = {
        waiting: {
          ...EMPTY_CHAT_BUCKET,
          replySessionsByPendingEventId: {
            pending: { pendingEventId: "pending", actorId: "actor", phase, updatedAt: Date.now() },
          },
        },
      };
      for (let i = 0; i < 20; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
      expect(buckets.waiting.replySessionsByPendingEventId.pending.phase).toBe(phase);
      expect(Object.keys(buckets)).toHaveLength(8);
    },
  );

  it("protects a queued local placeholder before a reply session exists", () => {
    let buckets: Record<string, GroupChatBucket> = {
      waiting: {
        ...EMPTY_CHAT_BUCKET,
        streamingEvents: [
          {
            id: "queued",
            kind: "chat.message",
            data: { pending_placeholder: true, stream_id: "local:queued" },
          },
        ],
      },
    };
    for (let i = 0; i < 20; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
    expect(buckets.waiting.streamingEvents[0].id).toBe("queued");
  });

  it.each(["completed", "failed"] as const)(
    "evicts %s sessions with retained placeholder records",
    (phase) => {
      let buckets: Record<string, GroupChatBucket> = {
        ended: {
          ...EMPTY_CHAT_BUCKET,
          streamingEvents: [
            {
              id: "old",
              kind: "chat.message",
              data: { stream_id: "s", pending_placeholder: true },
            },
          ],
          pendingEventIdByStreamId: { s: "pending" },
          replySessionsByPendingEventId: {
            pending: { pendingEventId: "pending", actorId: "actor", phase, updatedAt: Date.now() },
          },
        },
      };
      for (let i = 0; i < 8; i++) buckets = ensureGroupChatBucket(buckets, `g${i}`);
      expect(buckets.ended).toBeUndefined();
      expect(Object.keys(buckets)).toHaveLength(8);
    },
  );

  it("does not recreate unchanged chat maps for the most recent group", () => {
    const buckets = { active: { ...EMPTY_CHAT_BUCKET } };
    expect(ensureGroupChatBucket(buckets, "active")).toBe(buckets);
  });
});
