export interface TerminalSnapshotMetadata {
  bytes: number;
  cursor: number;
  cols: number | null;
  rows: number | null;
}

export interface TerminalAttachCursorPlan {
  deliveredCursor: number | null;
  receivedCursor: number | null;
  replayEndCursor: number | null;
  resetTerminal: boolean;
  snapshot: TerminalSnapshotMetadata | null;
}

export interface SnapshotTerminalViewport {
  readonly cols: number;
  readonly rows: number;
  resize: (cols: number, rows: number) => void;
  reset: () => void;
}

function finiteCursor(value: unknown): number | null {
  const cursor = Number(value);
  return Number.isFinite(cursor) && cursor >= 0 ? Math.floor(cursor) : null;
}

function optionalSnapshotDimension(value: unknown, minimum: number): number | null | undefined {
  if (value === undefined || value === null) return null;
  const dimension = finiteCursor(value);
  return dimension !== null && dimension >= minimum && dimension <= 4096 ? dimension : undefined;
}

export function planTerminalAttach(
  result: Record<string, unknown>,
  currentDeliveredCursor: number | null,
): TerminalAttachCursorPlan | null {
  const replayCursor = finiteCursor(result.replay_cursor);
  if (replayCursor === null) return null;

  const initial =
    result.initial_output && typeof result.initial_output === "object"
      ? (result.initial_output as Record<string, unknown>)
      : null;
  if (initial?.kind === "snapshot") {
    const bytes = finiteCursor(initial.bytes);
    const cursor = finiteCursor(initial.cursor);
    const replayEnd = finiteCursor(result.replay_end_cursor);
    const cols = optionalSnapshotDimension(initial.cols, 10);
    const rows = optionalSnapshotDimension(initial.rows, 2);
    if (
      bytes === null ||
      bytes <= 0 ||
      cursor !== replayCursor ||
      replayEnd !== cursor ||
      cols === undefined ||
      rows === undefined ||
      (cols === null) !== (rows === null)
    )
      return null;
    return {
      deliveredCursor: currentDeliveredCursor,
      receivedCursor: cursor,
      replayEndCursor: cursor,
      resetTerminal: true,
      snapshot: { bytes, cursor, cols, rows },
    };
  }

  const replayEnd = finiteCursor(result.replay_end_cursor);
  return {
    deliveredCursor: replayCursor,
    receivedCursor: replayCursor,
    replayEndCursor: replayEnd === null ? null : Math.max(replayCursor, replayEnd),
    resetTerminal: currentDeliveredCursor !== null && replayCursor !== currentDeliveredCursor,
    snapshot: null,
  };
}

export function prepareTerminalForSnapshot(
  terminal: SnapshotTerminalViewport,
  snapshot: TerminalSnapshotMetadata,
): boolean {
  const shouldRefit =
    snapshot.cols !== null &&
    snapshot.rows !== null &&
    (terminal.cols !== snapshot.cols || terminal.rows !== snapshot.rows);
  if (shouldRefit) terminal.resize(snapshot.cols!, snapshot.rows!);
  terminal.reset();
  return shouldRefit;
}
