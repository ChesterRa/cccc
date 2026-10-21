import type { GroupChatBucket } from "./groupStoreCore";
import type {
  Actor,
  GroupContext,
  GroupDoc,
  GroupPresentation,
  GroupSettings,
  LedgerEvent,
} from "../types";

export type GroupViewSnapshot = {
  groupDoc: GroupDoc | null;
  events: LedgerEvent[];
  actors: Actor[];
  groupContext: GroupContext | null;
  groupSettings: GroupSettings | null;
  groupPresentation: GroupPresentation | null;
  hasMoreHistory: boolean;
  cachedAt: number;
};

export const MAX_CACHED_GROUPS = 8;
export const GROUP_VIEW_CACHE_TTL_MS = 300_000;

/** Bounded snapshots: expiration removes references even when the UI is idle. */
export class GroupViewCache<T extends { cachedAt: number }> extends Map<string, T> {
  private timer: ReturnType<typeof setTimeout> | undefined;

  override get(key: string): T | undefined {
    const value = super.get(key);
    if (!value) return undefined;
    if (Date.now() - value.cachedAt >= GROUP_VIEW_CACHE_TTL_MS) {
      this.delete(key);
      return undefined;
    }
    // LRU reads do not renew the freshness deadline.
    super.delete(key);
    super.set(key, value);
    return value;
  }

  override set(key: string, value: T): this {
    this.removeExpired();
    super.delete(key);
    super.set(key, value);
    while (this.size > MAX_CACHED_GROUPS) super.delete(this.keys().next().value!);
    this.scheduleExpiration();
    return this;
  }

  override delete(key: string): boolean {
    const removed = super.delete(key);
    if (removed) this.scheduleExpiration();
    return removed;
  }

  override clear(): void {
    super.clear();
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  private removeExpired(): void {
    const now = Date.now();
    for (const [key, value] of this) {
      if (now - value.cachedAt >= GROUP_VIEW_CACHE_TTL_MS) super.delete(key);
    }
  }

  private scheduleExpiration(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    if (!this.size) return;
    const deadline = Math.min(...Array.from(this.values(), (value) => value.cachedAt));
    this.timer = setTimeout(
      () => {
        this.timer = undefined;
        this.removeExpired();
        this.scheduleExpiration();
      },
      Math.max(1, deadline + GROUP_VIEW_CACHE_TTL_MS - Date.now()),
    );
    // The cache alone must not keep Node test/SSR processes alive.
    if (typeof this.timer === "object") this.timer.unref?.();
  }
}

function hasActiveReply(bucket: GroupChatBucket): boolean {
  const sessions = bucket.replySessionsByPendingEventId;
  if (
    Object.values(sessions).some(
      (session) => session.phase === "pending" || session.phase === "streaming",
    )
  ) {
    return true;
  }
  return bucket.streamingEvents.some((event) => {
    const data = event.data as
      | { pending_event_id?: string; stream_id?: string; pending_placeholder?: boolean }
      | undefined;
    const pendingId =
      data?.pending_event_id || bucket.pendingEventIdByStreamId[data?.stream_id || ""];
    const session = sessions[pendingId];
    // Terminal sessions can retain output or placeholder records for display.
    if (session?.phase === "completed" || session?.phase === "failed") return false;
    return Boolean(event._streaming || data?.pending_placeholder);
  });
}

/** Preserve active work; old, reloadable conversations may be fetched again. */
export function retainRecentChatBuckets(
  buckets: Record<string, GroupChatBucket>,
  selectedGroupId: string,
  touchedGroupId = selectedGroupId,
): Record<string, GroupChatBucket> {
  const keys = Object.keys(buckets);
  if (keys.length <= MAX_CACHED_GROUPS) return buckets;
  const keep = new Set(
    keys.filter((key) => {
      const bucket = buckets[key];
      return (
        key === selectedGroupId ||
        key === touchedGroupId ||
        hasActiveReply(bucket) ||
        bucket.isLoadingHistory ||
        bucket.isChatWindowLoading
      );
    }),
  );
  for (const key of [...keys].reverse()) {
    if (keep.size >= MAX_CACHED_GROUPS) break;
    keep.add(key);
  }
  return Object.fromEntries(keys.filter((key) => keep.has(key)).map((key) => [key, buckets[key]]));
}

export function touchChatBucket(
  buckets: Record<string, GroupChatBucket>,
  groupId: string,
  bucket: GroupChatBucket,
): Record<string, GroupChatBucket> {
  const keys = Object.keys(buckets);
  if (keys.at(-1) === groupId && keys.length <= MAX_CACHED_GROUPS) return buckets;
  const next = { ...buckets };
  delete next[groupId];
  next[groupId] = bucket;
  return retainRecentChatBuckets(next, groupId);
}
