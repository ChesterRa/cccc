import { memo, useEffect, useMemo, useState } from "react";

import { classNames } from "../../utils/classNames";
import {
  areRuntimeDockTickerEntriesEqual,
  hasRuntimeDockTickerWork,
  pruneRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
} from "./runtimeDockTickerCache";
import { runtimeDockTickerCacheRegistry } from "./runtimeDockTickerCacheRegistry";
import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

type RuntimeDockTickerProps = {
  groupId: string;
  entries: RuntimeDockTickerEntry[];
  isDark: boolean;
  suppressed: boolean;
};

function RuntimeDockTickerView({ groupId, entries, isDark, suppressed }: RuntimeDockTickerProps) {
  const cache = useMemo(() => runtimeDockTickerCacheRegistry.get(groupId), [groupId]);
  const [visibleEntries, setVisibleEntries] = useState<RuntimeDockTickerEntry[]>([]);
  const [tickerWorkPending, setTickerWorkPending] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const nextEntries = upsertRuntimeDockTickerCache(cache, entries, Date.now());
      setVisibleEntries((current) =>
        areRuntimeDockTickerEntriesEqual(current, nextEntries) ? current : nextEntries,
      );
      setTickerWorkPending(hasRuntimeDockTickerWork(cache));
    }, 0);
    return () => window.clearTimeout(timer);
  }, [cache, entries]);

  useEffect(() => {
    if (!tickerWorkPending) return;
    const timer = window.setInterval(() => {
      const nextEntries = pruneRuntimeDockTickerCache(cache, Date.now());
      setVisibleEntries((current) =>
        areRuntimeDockTickerEntriesEqual(current, nextEntries) ? current : nextEntries,
      );
      setTickerWorkPending(hasRuntimeDockTickerWork(cache));
    }, 250);
    return () => window.clearInterval(timer);
  }, [cache, tickerWorkPending]);

  if (visibleEntries.length <= 0) return null;
  return (
    <div
      className={classNames(
        "pointer-events-none absolute bottom-[calc(100%+0.8rem)] left-1/2 z-20 h-[5.25rem] w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden transition-opacity duration-200 ease-out",
        suppressed ? "invisible opacity-0" : "visible opacity-100",
      )}
      style={{
        WebkitMaskImage:
          "linear-gradient(to top, #000 0%, #000 72%, rgba(0,0,0,0.78) 84%, transparent 100%)",
        maskImage:
          "linear-gradient(to top, #000 0%, #000 72%, rgba(0,0,0,0.78) 84%, transparent 100%)",
      }}
      aria-hidden="true"
    >
      <div className="absolute inset-x-0 bottom-0 flex flex-col items-center gap-1 px-1">
        {visibleEntries.map((entry, index) => {
          const slotFromLatest = visibleEntries.length - index - 1;
          const isMessage = entry.kind === "message";
          return (
            <div
              key={entry.id}
              className={classNames(
                "runtime-dock-ticker-entry break-words border shadow-[0_14px_36px_-30px_rgba(15,23,42,0.55)] backdrop-blur-xl transition-opacity duration-500 ease-out motion-reduce:animate-none motion-reduce:transition-none",
                isMessage
                  ? "w-fit min-w-0 max-w-[min(84vw,380px)] rounded-2xl px-3 py-1.5 text-left text-[11px] leading-[1.28] whitespace-pre-wrap [overflow-wrap:anywhere] hyphens-auto"
                  : "w-fit max-w-full rounded-full px-2.5 py-1 text-[11px] leading-[1.15] whitespace-pre-wrap",
                slotFromLatest === 0
                  ? "opacity-100"
                  : slotFromLatest === 1
                    ? "opacity-[0.66]"
                    : slotFromLatest === 2
                      ? "opacity-[0.38]"
                      : "opacity-[0.2]",
                isDark
                  ? "border-white/[0.08] bg-slate-950/70 text-slate-200"
                  : "border-black/[0.08] bg-white/[0.78] text-gray-700",
              )}
            >
              <span
                className={classNames(
                  "font-semibold",
                  isDark ? "text-white" : "text-[rgb(35,36,37)]",
                )}
              >
                {entry.actorLabel}
              </span>
              <span className={isDark ? "text-slate-500" : "text-gray-400"}>: </span>
              <span>{entry.text}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export const RuntimeDockTicker = memo(
  RuntimeDockTickerView,
  (previous, next) =>
    previous.groupId === next.groupId &&
    previous.isDark === next.isDark &&
    previous.suppressed === next.suppressed &&
    areRuntimeDockTickerEntriesEqual(previous.entries, next.entries),
);
