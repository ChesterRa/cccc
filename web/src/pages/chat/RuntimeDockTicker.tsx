import { memo } from "react";

import { classNames } from "../../utils/classNames";
import { useRuntimeDockTickerEntries } from "./useRuntimeDockTickerEntries";
import { areRuntimeDockTickerEntriesEqual } from "./runtimeDockTickerCache";
import type { RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";

type RuntimeDockTickerProps = {
  groupId: string;
  entries: RuntimeDockTickerEntry[];
  suppressed: boolean;
};

function RuntimeDockTickerView({ groupId, entries, suppressed }: RuntimeDockTickerProps) {
  const visibleEntries = useRuntimeDockTickerEntries(groupId, entries, suppressed);

  if (visibleEntries.length <= 0) return null;
  return (
    <div
      className={classNames(
        "pointer-events-none z-20 mb-1 w-full md:absolute md:bottom-[calc(100%+0.8rem)] md:left-1/2 md:mb-0 md:w-[min(92vw,560px)] md:-translate-x-1/2",
        suppressed ? "invisible" : "visible",
      )}
      aria-hidden="true"
    >
      <div className="flex flex-col items-center gap-1 px-1">
        {visibleEntries.map((entry, index) => {
          const slotFromLatest = visibleEntries.length - index - 1;
          const isMessage = entry.kind === "message";
          return (
            <div
              key={entry.id}
              // A framed line on the page ground; older lines step back.
              className={classNames(
                "hidden last:block md:block break-words border border-[var(--color-border-secondary)] bg-[var(--color-bg-primary)]",
                isMessage
                  ? "w-fit min-w-0 max-w-[min(84vw,380px)] px-3 py-1.5 text-left text-[11px] leading-[1.28] whitespace-pre-wrap [overflow-wrap:anywhere] hyphens-auto"
                  : "w-fit max-w-full px-2.5 py-1 text-[11px] leading-[1.15] whitespace-pre-wrap",
                slotFromLatest === 0
                  ? "text-[var(--color-text-secondary)]"
                  : "text-[var(--color-text-tertiary)]",
              )}
            >
              <div className="line-clamp-1 md:line-clamp-2">
                <span className="font-semibold text-[var(--color-text-primary)]">
                  {entry.actorLabel}
                </span>
                <span className="text-[var(--color-text-tertiary)]">: </span>
                <span>{entry.text}</span>
              </div>
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
    previous.suppressed === next.suppressed &&
    areRuntimeDockTickerEntriesEqual(previous.entries, next.entries),
);
