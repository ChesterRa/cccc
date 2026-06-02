import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

type CachedRuntimeDockTickerEntry = RuntimeDockTickerEntry & {
  displayMs: number;
  lastUpdatedAtMs: number;
  signature: string;
};

type MessageSourceState = {
  consumedText: string;
  revision: number;
  sequence: number;
  pendingText: string;
  pendingStartedAtMs: number;
  lastFlushedAtMs: number;
  drainingBacklog: boolean;
  lastEntry: RuntimeDockTickerEntry | null;
};

export type RuntimeDockTickerCache = {
  entries: Map<string, CachedRuntimeDockTickerEntry>;
  messageSources: Map<string, MessageSourceState>;
  retiredSignatures: Map<string, string>;
};

const TICKER_HOLD_AFTER_DISPLAY_MS = 5000;
const TICKER_CACHE_LIMIT = 96;
const TICKER_RENDER_LIMIT = 24;
const TICKER_MESSAGE_CHUNK_CHAR_LIMIT = 96;
const TICKER_MESSAGE_CHUNK_SOFT_MIN_CHARS = 56;
const TICKER_MESSAGE_CHUNK_REVEAL_INTERVAL_MS = 250;
const TICKER_MESSAGE_FLUSH_DELAY_MS = 420;
const TICKER_MESSAGE_MIN_BOUNDARY_FLUSH_CHARS = 18;
const TICKER_MESSAGE_MIN_TIMED_FLUSH_CHARS = 12;
const TICKER_MESSAGE_DISPLAY_LINE_CHAR_ESTIMATE = 42;
const TICKER_MESSAGE_EXTRA_DISPLAY_MS_PER_LINE = 900;
const TICKER_MESSAGE_SOURCE_LIMIT = 120;
const TICKER_RETIRED_SIGNATURE_LIMIT = 160;

export function createRuntimeDockTickerCache(): RuntimeDockTickerCache {
  return {
    entries: new Map(),
    messageSources: new Map(),
    retiredSignatures: new Map(),
  };
}

export function hasRuntimeDockTickerWork(cache: RuntimeDockTickerCache): boolean {
  if (cache.entries.size > 0) return true;
  for (const source of cache.messageSources.values()) {
    const pendingText = String(source.pendingText || "").trim();
    if (!pendingText) continue;
    if (source.drainingBacklog || source.lastEntry?.completed) return true;
    const length = getTickerMessageLength(pendingText);
    if (length >= TICKER_MESSAGE_CHUNK_CHAR_LIMIT) return true;
    if (hasTickerMessageHardBoundary(pendingText)) return true;
    if (length >= TICKER_MESSAGE_MIN_BOUNDARY_FLUSH_CHARS && hasTickerMessageSoftBoundary(pendingText)) return true;
    if (length >= TICKER_MESSAGE_MIN_TIMED_FLUSH_CHARS) return true;
  }
  return false;
}

function getTickerTimestampMs(value: string): number | null {
  const timestamp = String(value || "").trim();
  if (!timestamp) return null;
  const parsed = Date.parse(timestamp);
  return Number.isFinite(parsed) ? parsed : null;
}

function getTickerEntrySignature(entry: RuntimeDockTickerEntry): string {
  return [entry.updatedAt, entry.text].join("\u0000");
}

function getMessageSourceId(entry: RuntimeDockTickerEntry): string {
  return String(entry.sourceId || entry.id || "").trim();
}

function splitTickerMessageDelta(value: string): string[] {
  const chunks: string[] = [];
  for (const rawLine of String(value || "").replace(/\r\n/g, "\n").split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;
    const chars = Array.from(line);
    let index = 0;
    while (index < chars.length) {
      const maxEnd = Math.min(chars.length, index + TICKER_MESSAGE_CHUNK_CHAR_LIMIT);
      let end = maxEnd;
      if (maxEnd < chars.length) {
        for (let cursor = maxEnd; cursor > index + TICKER_MESSAGE_CHUNK_SOFT_MIN_CHARS; cursor -= 1) {
          if (/[\s,，、.!?。！？;；:：)\]}]/.test(chars[cursor - 1] || "")) {
            end = cursor;
            break;
          }
        }
      }
      const chunk = chars.slice(index, end).join("").trim();
      if (chunk.trim()) chunks.push(chunk);
      index = end;
      while (index < chars.length && /\s/.test(chars[index] || "")) {
        index += 1;
      }
    }
  }
  return chunks;
}

function getTickerMessageLength(value: string): number {
  return Array.from(String(value || "").trim()).length;
}

function hasTickerMessageHardBoundary(value: string): boolean {
  const text = String(value || "");
  return text.includes("\n") || /(?:[.!?。！？;；:：])\s*$/.test(text);
}

function hasTickerMessageSoftBoundary(value: string): boolean {
  return /(?:[,，、)])\s*$/.test(String(value || ""));
}

function shouldFlushTickerMessage(value: string, startedAtMs: number, nowMs: number, completed = false): boolean {
  const length = getTickerMessageLength(value);
  if (length <= 0) return false;
  if (completed) return true;
  if (length >= TICKER_MESSAGE_CHUNK_CHAR_LIMIT) return true;
  if (hasTickerMessageHardBoundary(value)) return true;
  if (length >= TICKER_MESSAGE_MIN_BOUNDARY_FLUSH_CHARS && hasTickerMessageSoftBoundary(value)) return true;
  if (length >= TICKER_MESSAGE_MIN_TIMED_FLUSH_CHARS && nowMs - startedAtMs >= TICKER_MESSAGE_FLUSH_DELAY_MS) return true;
  return false;
}

function getTickerMessageDisplayMs(value: string): number {
  const estimatedLines = Math.max(1, Math.ceil(getTickerMessageLength(value) / TICKER_MESSAGE_DISPLAY_LINE_CHAR_ESTIMATE));
  return TICKER_HOLD_AFTER_DISPLAY_MS + (estimatedLines - 1) * TICKER_MESSAGE_EXTRA_DISPLAY_MS_PER_LINE;
}

function compareCachedTickerEntries(left: CachedRuntimeDockTickerEntry, right: CachedRuntimeDockTickerEntry): number {
  const leftTs = getTickerTimestampMs(left.updatedAt) ?? left.lastUpdatedAtMs;
  const rightTs = getTickerTimestampMs(right.updatedAt) ?? right.lastUpdatedAtMs;
  if (leftTs !== rightTs) return leftTs - rightTs;
  return left.id.localeCompare(right.id);
}

function trimRetiredSignatures(cache: RuntimeDockTickerCache): void {
  const overflow = cache.retiredSignatures.size - TICKER_RETIRED_SIGNATURE_LIMIT;
  if (overflow <= 0) return;
  for (const id of Array.from(cache.retiredSignatures.keys()).slice(0, overflow)) {
    cache.retiredSignatures.delete(id);
  }
}

function trimMessageSources(cache: RuntimeDockTickerCache): void {
  const overflow = cache.messageSources.size - TICKER_MESSAGE_SOURCE_LIMIT;
  if (overflow <= 0) return;
  for (const id of Array.from(cache.messageSources.keys()).slice(0, overflow)) {
    cache.messageSources.delete(id);
  }
}

function retireTickerEntry(cache: RuntimeDockTickerCache, entry: CachedRuntimeDockTickerEntry): void {
  cache.retiredSignatures.set(entry.id, entry.signature);
  trimRetiredSignatures(cache);
}

function toVisibleEntries(cache: RuntimeDockTickerCache): RuntimeDockTickerEntry[] {
  return Array.from(cache.entries.values())
    .sort(compareCachedTickerEntries)
    .slice(-TICKER_RENDER_LIMIT)
    .map(({ displayMs: _displayMs, lastUpdatedAtMs: _lastUpdatedAtMs, signature: _signature, ...entry }) => entry);
}

export function pruneRuntimeDockTickerCache(cache: RuntimeDockTickerCache, nowMs: number): RuntimeDockTickerEntry[] {
  flushPendingMessageSources(cache, nowMs);

  for (const [id, entry] of cache.entries.entries()) {
    if (nowMs - entry.lastUpdatedAtMs > entry.displayMs) {
      cache.entries.delete(id);
      retireTickerEntry(cache, entry);
    }
  }

  const sortedEntries = Array.from(cache.entries.values()).sort(compareCachedTickerEntries);
  const overflow = sortedEntries.length - TICKER_CACHE_LIMIT;
  if (overflow > 0) {
    for (const entry of sortedEntries.slice(0, overflow)) {
      cache.entries.delete(entry.id);
      retireTickerEntry(cache, entry);
    }
  }

  return toVisibleEntries(cache);
}

function upsertStableTickerEntry(
  cache: RuntimeDockTickerCache,
  entry: RuntimeDockTickerEntry,
  nowMs: number,
  displayMs = TICKER_HOLD_AFTER_DISPLAY_MS,
): void {
  const signature = getTickerEntrySignature(entry);
  const existing = cache.entries.get(entry.id);
  if (!existing && cache.retiredSignatures.get(entry.id) === signature) {
    return;
  }
  cache.retiredSignatures.delete(entry.id);
  cache.entries.set(entry.id, {
    ...entry,
    displayMs,
    lastUpdatedAtMs: signature === existing?.signature ? existing.lastUpdatedAtMs : nowMs,
    signature,
  });
}

function flushPendingMessageSource(
  cache: RuntimeDockTickerCache,
  sourceId: string,
  source: MessageSourceState,
  nowMs: number,
  force = false,
): MessageSourceState {
  const pendingText = String(source.pendingText || "");
  if (!pendingText.trim()) {
    return { ...source, pendingText: "", pendingStartedAtMs: 0, drainingBacklog: false };
  }
  const pendingStartedAtMs = source.pendingStartedAtMs || nowMs;
  if (!force && source.lastFlushedAtMs > 0 && nowMs - source.lastFlushedAtMs < TICKER_MESSAGE_CHUNK_REVEAL_INTERVAL_MS) {
    return { ...source, pendingStartedAtMs };
  }
  const baseEntry = source.lastEntry;
  if (!baseEntry) {
    return { ...source, pendingText: "", pendingStartedAtMs: 0 };
  }
  if (!force && !source.drainingBacklog && !shouldFlushTickerMessage(pendingText, pendingStartedAtMs, nowMs, Boolean(baseEntry.completed))) {
    return { ...source, pendingStartedAtMs };
  }

  const [chunk, ...remainingChunks] = splitTickerMessageDelta(pendingText);
  if (!chunk) {
    return { ...source, pendingText: "", pendingStartedAtMs: 0, drainingBacklog: false };
  }
  const sequence = source.sequence + 1;
  const backlogDisplayMs = remainingChunks.length * TICKER_MESSAGE_CHUNK_REVEAL_INTERVAL_MS;
  upsertStableTickerEntry(cache, {
    ...baseEntry,
    id: [sourceId, "rev", String(source.revision).padStart(3, "0"), "seq", String(sequence).padStart(5, "0")].join(":"),
    text: chunk,
    sourceId,
  }, nowMs, getTickerMessageDisplayMs(chunk) + backlogDisplayMs);

  return {
    ...source,
    sequence,
    pendingText: remainingChunks.join("\n"),
    pendingStartedAtMs: remainingChunks.length > 0 ? nowMs - TICKER_MESSAGE_FLUSH_DELAY_MS : 0,
    lastFlushedAtMs: nowMs,
    drainingBacklog: remainingChunks.length > 0,
  };
}

function flushPendingMessageSources(cache: RuntimeDockTickerCache, nowMs: number): void {
  for (const [sourceId, source] of cache.messageSources.entries()) {
    const nextSource = flushPendingMessageSource(cache, sourceId, source, nowMs);
    if (nextSource !== source) {
      cache.messageSources.set(sourceId, nextSource);
    }
  }
}

function upsertMessageTickerEntry(
  cache: RuntimeDockTickerCache,
  entry: RuntimeDockTickerEntry,
  nowMs: number,
): void {
  const sourceId = getMessageSourceId(entry);
  if (!sourceId) return;
  const nextText = String(entry.text || "").trim();
  if (!nextText) return;

  const previousSource = cache.messageSources.get(sourceId);
  let source: MessageSourceState = previousSource
    ? { ...previousSource, lastEntry: entry }
    : {
        consumedText: "",
        revision: 0,
        sequence: 0,
        pendingText: "",
        pendingStartedAtMs: 0,
        lastFlushedAtMs: 0,
        drainingBacklog: false,
        lastEntry: entry,
      };
  let delta = nextText;

  if (previousSource) {
    if (nextText === previousSource.consumedText) {
      cache.messageSources.set(sourceId, flushPendingMessageSource(cache, sourceId, source, nowMs));
      return;
    }
    if (nextText.startsWith(previousSource.consumedText)) {
      delta = nextText.slice(previousSource.consumedText.length);
    } else {
      source = {
        ...source,
        revision: source.revision + 1,
        sequence: 0,
        pendingText: "",
        pendingStartedAtMs: 0,
        lastFlushedAtMs: 0,
        drainingBacklog: false,
      };
    }
  }

  const nextSource = flushPendingMessageSource(cache, sourceId, {
    ...source,
    consumedText: nextText,
    pendingText: `${source.pendingText || ""}${delta}`,
    pendingStartedAtMs: source.pendingText ? (source.pendingStartedAtMs || nowMs) : nowMs,
    lastEntry: entry,
  }, nowMs);

  cache.messageSources.set(sourceId, nextSource);
  trimMessageSources(cache);
}

export function upsertRuntimeDockTickerCache(
  cache: RuntimeDockTickerCache,
  entries: RuntimeDockTickerEntry[],
  nowMs: number,
): RuntimeDockTickerEntry[] {
  for (const entry of entries) {
    if (entry.kind === "message") {
      upsertMessageTickerEntry(cache, entry, nowMs);
    } else {
      upsertStableTickerEntry(cache, entry, nowMs);
    }
  }
  return pruneRuntimeDockTickerCache(cache, nowMs);
}
