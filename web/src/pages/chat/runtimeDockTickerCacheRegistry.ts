import {
  createRuntimeDockTickerCache,
  type RuntimeDockTickerCache,
} from "./runtimeDockTickerCache";

export type RuntimeDockTickerCacheRegistry = { get: (groupId: string) => RuntimeDockTickerCache };

const DEFAULT_GROUP_CACHE_LIMIT = 32;

export function createRuntimeDockTickerCacheRegistry(
  limit = DEFAULT_GROUP_CACHE_LIMIT,
): RuntimeDockTickerCacheRegistry {
  const caches = new Map<string, RuntimeDockTickerCache>();
  const boundedLimit = Math.max(1, Math.floor(limit));

  return {
    get(groupId: string): RuntimeDockTickerCache {
      const key = String(groupId || "").trim();
      const existing = caches.get(key);
      if (existing) {
        caches.delete(key);
        caches.set(key, existing);
        return existing;
      }

      const cache = createRuntimeDockTickerCache();
      caches.set(key, cache);
      while (caches.size > boundedLimit) {
        const oldestKey = caches.keys().next().value;
        if (oldestKey === undefined) break;
        caches.delete(oldestKey);
      }
      return cache;
    },
  };
}

export const runtimeDockTickerCacheRegistry = createRuntimeDockTickerCacheRegistry();
