import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

type CachedEntry = { entry: RuntimeDockTickerEntry; signature: string; expiresAt: number };
export type RuntimeDockTickerCache = { entries: Map<string, CachedEntry> };
const HOLD_MS = 6000;
const ACTOR_LIMIT = 96;
const VISIBLE_LIMIT = 2;
const EXCERPT_CHAR_LIMIT = 120;

export function createRuntimeDockTickerCache(): RuntimeDockTickerCache {
  return { entries: new Map() };
}

export function areRuntimeDockTickerEntriesEqual(
  left: RuntimeDockTickerEntry[],
  right: RuntimeDockTickerEntry[],
): boolean {
  return (
    left === right ||
    (left.length === right.length &&
      left.every((entry, index) => {
        const other = right[index];
        return (
          other &&
          entry.id === other.id &&
          entry.kind === other.kind &&
          entry.actorLabel === other.actorLabel &&
          entry.text === other.text &&
          entry.sourceId === other.sourceId &&
          entry.receivedAt === other.receivedAt &&
          entry.completed === other.completed
        );
      }))
  );
}

function excerpt(value: string): string {
  const line =
    value
      .split(/\r?\n/)
      .map((part) => part.trim())
      .filter(Boolean)
      .at(-1) || "";
  const chars = Array.from(line.replace(/^[-*#>]\s+/, "").replace(/\s+/g, " "));
  return chars.length > EXCERPT_CHAR_LIMIT
    ? `…${chars.slice(-(EXCERPT_CHAR_LIMIT - 1)).join("")}`
    : chars.join("");
}

export function hasRuntimeDockTickerWork(cache: RuntimeDockTickerCache): boolean {
  return Array.from(cache.entries.values()).some((entry) => entry.expiresAt > 0);
}

export function pruneRuntimeDockTickerCache(
  cache: RuntimeDockTickerCache,
  nowMs: number,
): RuntimeDockTickerEntry[] {
  for (const cached of cache.entries.values()) {
    if (cached.expiresAt <= nowMs) cached.expiresAt = 0;
  }
  return Array.from(cache.entries.values())
    .filter((entry) => entry.expiresAt > nowMs)
    .sort((a, b) => a.entry.receivedAt - b.entry.receivedAt)
    .slice(-VISIBLE_LIMIT)
    .map((cached) => cached.entry);
}

export function upsertRuntimeDockTickerCache(
  cache: RuntimeDockTickerCache,
  entries: RuntimeDockTickerEntry[],
  nowMs: number,
): RuntimeDockTickerEntry[] {
  for (const incoming of entries) {
    if (!incoming.receivedAt || incoming.receivedAt + HOLD_MS <= nowMs) continue;
    const text = excerpt(incoming.text);
    if (!text) continue;
    const signature = `${incoming.sourceId || incoming.id}\0${text}`;
    const previous = cache.entries.get(incoming.actorId);
    // Completion of the same prose or a re-projection cannot restart its display timer.
    if (previous?.signature === signature) continue;
    if (previous && previous.entry.receivedAt > incoming.receivedAt) continue;
    cache.entries.delete(incoming.actorId);
    cache.entries.set(incoming.actorId, {
      entry: { ...incoming, id: incoming.actorId, text },
      signature,
      expiresAt: incoming.receivedAt + HOLD_MS,
    });
  }
  while (cache.entries.size > ACTOR_LIMIT) cache.entries.delete(cache.entries.keys().next().value!);
  return pruneRuntimeDockTickerCache(cache, nowMs);
}
