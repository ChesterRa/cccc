import { classNames } from "../../utils/classNames";

// Chet's head: a solid disc in the LCD's ink with two small square eyes cut out of it. Each size
// has its own grid, 16×16 for the 16 px places and 32×32 for the 32 px places, so neither is a
// scaled copy of the other.
type Grid = 16 | 32;
type Frame = readonly string[];

const EYES: Record<
  Grid,
  {
    /** The left eye's square, whose mirror is the right eye. */
    x: number;
    y: number;
    size: number;
    /** How far the eyes move when Chet glances aside at work. */
    glance: number;
  }
> = { 16: { x: 4, y: 6, size: 2, glance: 1 }, 32: { x: 8, y: 11, size: 4, glance: 2 } };

/** One frame as rows of "#" (ink) and "." (screen). A closed eye is one row at the eye's middle. */
function face(grid: Grid, look = 0, shut = false): Frame {
  const { x: eyeX, y: eyeY, size, glance } = EYES[grid];
  const radius = grid / 2;
  const shift = look * glance;
  const eyeTop = shut ? eyeY + size / 2 : eyeY;
  const eyeBottom = shut ? eyeTop + 1 : eyeY + size;
  const inEye = (x: number, y: number) =>
    y >= eyeTop &&
    y < eyeBottom &&
    [eyeX + shift, grid - eyeX - size + shift].some((left) => x >= left && x < left + size);
  // A cell is in the disc when its centre is.
  const inDisc = (x: number, y: number) =>
    (x + 0.5 - radius) ** 2 + (y + 0.5 - radius) ** 2 <= radius ** 2;
  return Array.from({ length: grid }, (_, y) =>
    Array.from({ length: grid }, (_, x) => (inDisc(x, y) && !inEye(x, y) ? "#" : ".")).join(""),
  );
}

/** One path of 1×1 cells, a run of cells in a row as one rectangle. */
function cells(frame: Frame): string {
  let d = "";
  frame.forEach((row, y) => {
    for (let x = 0; x < row.length; ) {
      if (row[x] !== "#") {
        x += 1;
        continue;
      }
      const start = x;
      while (x < row.length && row[x] === "#") x += 1;
      d += `M${start} ${y}h${x - start}v1h-${x - start}z`;
    }
  });
  return d;
}

function frames(grid: Grid) {
  return [
    { name: "rest", d: cells(face(grid)) },
    { name: "blink", d: cells(face(grid, 0, true)) },
    { name: "work", d: cells(face(grid, -1)) },
    { name: "glance", d: cells(face(grid, 1)) },
  ];
}

const FRAMES: Record<Grid, ReturnType<typeof frames>> = { 16: frames(16), 32: frames(32) };

/**
 * Chet's face adds character; the status text, the alert mark and the call controls
 * remain the record of state. Idle, it blinks now and then; while Chet works, its eyes
 * glance left and right. The frames swap in whole steps (lcdMotion.css) and hold still
 * when motion is off. Their paths inherit the screen's ink from the root's CSS fill.
 */
export function ChetHead({
  size,
  working = false,
  className,
}: {
  size: Grid;
  working?: boolean;
  className?: string;
}) {
  return (
    <svg
      viewBox={`0 0 ${size} ${size}`}
      width={size}
      height={size}
      shapeRendering="crispEdges"
      aria-hidden="true"
      focusable="false"
      className={classNames("chet-head flex-none", className)}
      style={{ fill: "var(--lcd-ink)" }}
      data-chet-head={working ? "working" : "idle"}
    >
      {FRAMES[size].map((frame) => (
        <g key={frame.name} data-frame={frame.name}>
          <path d={frame.d} />
        </g>
      ))}
    </svg>
  );
}
