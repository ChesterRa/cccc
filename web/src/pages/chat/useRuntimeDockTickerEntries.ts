import { useEffect, useMemo, useState } from "react";
import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";
import { runtimeDockTickerCacheRegistry } from "./runtimeDockTickerCacheRegistry";
import {
  areRuntimeDockTickerEntriesEqual,
  nextRuntimeDockTickerExpiry,
  upsertRuntimeDockTickerCache,
} from "./runtimeDockTickerCache";

export function useRuntimeDockTickerEntries(
  groupId: string,
  entries: RuntimeDockTickerEntry[],
  suppressed: boolean,
): RuntimeDockTickerEntry[] {
  const cache = useMemo(() => runtimeDockTickerCacheRegistry.get(groupId), [groupId]);
  const [visibleEntries, setVisibleEntries] = useState<RuntimeDockTickerEntry[]>([]);

  useEffect(() => {
    let timer: number | undefined;
    const cancel = () => {
      if (timer !== undefined) window.clearTimeout(timer);
      timer = undefined;
    };
    const refresh = () => {
      cancel();
      // Incoming data remains in the store; resume from its original timestamps.
      if (document.hidden || suppressed) return;
      const now = Date.now();
      const next = upsertRuntimeDockTickerCache(cache, entries, now);
      setVisibleEntries((current) =>
        areRuntimeDockTickerEntriesEqual(current, next) ? current : next,
      );
      const expiry = nextRuntimeDockTickerExpiry(cache);
      if (expiry !== null) timer = window.setTimeout(refresh, Math.max(1, expiry - now));
    };
    // Keep the initial projection outside the synchronous effect body.
    if (!document.hidden && !suppressed) timer = window.setTimeout(refresh, 0);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      cancel();
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [cache, entries, suppressed]);

  return suppressed ? [] : visibleEntries;
}
