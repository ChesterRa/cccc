/**
 * A PTY resize makes full-screen TUIs (Codex, Claude) repaint the whole screen.
 * While the mobile keyboard slides, the terminal is refit through intermediate
 * sizes; forwarding each one repaints several times and the screen flickers.
 * Only the size that holds for `settleMs` reaches the PTY; xterm itself still
 * refits immediately so content is never clipped.
 */
export function createTerminalResizeCoalescer(
  send: (cols: number, rows: number) => void,
  settleMs = 150,
) {
  let pending: { cols: number; rows: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    push(cols: number, rows: number) {
      pending = { cols, rows };
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        const next = pending;
        pending = null;
        if (next) send(next.cols, next.rows);
      }, settleMs);
    },
    dispose() {
      if (timer) clearTimeout(timer);
      timer = null;
      pending = null;
    },
  };
}
