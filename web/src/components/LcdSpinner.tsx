import { useEffect, useState, type ReactNode } from "react";

import { classNames } from "../utils/classNames";

/** The 3x3 grid's outer cells, clockwise from the top left; the centre stays empty. */
const CELLS: ReadonlyArray<readonly [number, number]> = [
  [0, 0],
  [1, 0],
  [2, 0],
  [2, 1],
  [2, 2],
  [1, 2],
  [0, 2],
  [0, 1],
];

/**
 * The working mark: a lit cell and its fading tail step round a 3x3 grid,
 * one cell every 125 ms. A short wait shows nothing: the cells appear after `delayMs`.
 * Until then the caller's own icon (`children`) stays, or an empty box of the mark's
 * size, so nothing beside it moves. Callers put the state into their label and disabled
 * state at once; the mark only shows that the work continues.
 */
export function LcdSpinner({
  className,
  delayMs = 250,
  children,
}: {
  className?: string;
  delayMs?: number;
  children?: ReactNode;
}) {
  const [shown, setShown] = useState(delayMs <= 0);
  useEffect(() => {
    if (delayMs <= 0) return;
    const timer = window.setTimeout(() => setShown(true), delayMs);
    return () => window.clearTimeout(timer);
  }, [delayMs]);
  if (!shown && children) return <>{children}</>;
  return (
    <svg
      className={classNames("lcd-spinner", className)}
      viewBox="0 0 3 3"
      width="1em"
      height="1em"
      data-shown={shown || undefined}
      aria-hidden="true"
      focusable="false"
    >
      {CELLS.map(([x, y], index) => (
        <rect
          key={index}
          x={x}
          y={y}
          width="1"
          height="1"
          // Each cell lights when the head reaches it: cell 0 now, cell 1 one step later.
          style={{ animationDelay: `${-((8 - index) % 8) * 125}ms` }}
        />
      ))}
    </svg>
  );
}
